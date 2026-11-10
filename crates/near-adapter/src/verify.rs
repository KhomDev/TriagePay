//! Receipt-level payment verification.
//!
//! A NEAR payment is a tree of receipts. A direct `ft_transfer` signed by
//! the payer executes in one receipt on the token contract; a payment
//! made through a policy-wallet contract arrives as a cross-contract
//! receipt whose predecessor is the wallet. Inspecting only the
//! transaction's top-level actions therefore rejects wallet payments,
//! and trusting any `EVENT_JSON` log accepts forgeries. This module
//! verifies the token contract's own NEP-141 transfer event inside the
//! finalized receipt tree instead.
//!
//! A payment confirms an intent only when ALL of these hold:
//!
//! 1. the transaction is final (`final_execution_status == FINAL` when
//!    reported) and its top-level status is `SuccessValue`;
//! 2. a receipt reachable from the transaction was executed by the
//!    intent's asset contract (`executor_id == asset`), and that
//!    receipt's own status is a success;
//! 3. that receipt's body (from `receipts`) carries a `FunctionCall`
//!    named `ft_transfer` with exactly 1 yoctoNEAR attached, and no
//!    `ft_transfer_call` (whose amount the token may refund later);
//! 4. its logs contain exactly one parseable NEP-297 `nep141`
//!    `ft_transfer` event entry with `new_owner_id == gateway` and
//!    `memo == intent_id`, whose `amount` parses as u128.
//!
//! Logs written under any other executor are ignored: any contract can
//! log a perfect-looking `EVENT_JSON` line, and only code running as the
//! token account can write one under the token's executor id. That
//! binding is the spoof-resistance property.
//!
//! The predecessor is deliberately not constrained: the payer itself
//! (direct transfer) and a policy-wallet contract (`pay`) both verify.
//! The event's `old_owner_id` is recorded as the payer; it is evidence,
//! not authorization.
//!
//! An intent is funded by at most one transfer. A second matching
//! transfer, in the same transaction or another one, becomes an orphan
//! deposit for operator-approved refund, never extra credit.

use std::collections::{HashMap, HashSet};

use aps_payment::{DepositIntent, ProviderError};
use serde_json::Value;

use crate::provider::{TxHint, Verification};
use crate::rpc::{
    NearRpc, OutcomeStatus, ReceiptOutcome, ReceiptView, TxAction, TxOutcome, TxStatus,
};

/// Prefix of a NEP-297 event log line.
pub const EVENT_PREFIX: &str = "EVENT_JSON:";

/// Facts about the token receipt that funded an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferEvidence {
    /// The transaction hash the payer presented.
    pub tx_hash: String,
    /// The token receipt carrying the funding event.
    pub receipt_id: String,
    /// Always the asset contract (the executor binding).
    pub executor_id: String,
    /// Who sent the token receipt: the payer for a direct transfer, the
    /// wallet contract for a policy-wallet payment.
    pub predecessor_id: String,
    /// The event's `old_owner_id`: the account whose tokens moved.
    pub payer_id: String,
    /// The event's `new_owner_id` (the gateway).
    pub receiver_id: String,
    /// The event's `amount`, in atomic units.
    pub amount_atomic: String,
    /// The event's `memo` (the intent id).
    pub memo: String,
}

/// A transfer that matched an intent's gateway and memo but did not fund
/// it. Orphans are refunded only by an operator-approved transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanTransfer {
    pub tx_hash: String,
    pub receipt_id: String,
    pub payer_id: String,
    pub amount_atomic: String,
    pub reason: String,
}

/// The full result of a receipt-level check: the typed verification plus
/// the evidence behind a confirmation and any orphaned transfers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptCheck {
    pub verification: Verification,
    /// Present exactly when `verification` is `Confirmed`.
    pub evidence: Option<TransferEvidence>,
    pub orphans: Vec<OrphanTransfer>,
}

impl ReceiptCheck {
    fn only(verification: Verification) -> Self {
        Self {
            verification,
            evidence: None,
            orphans: Vec::new(),
        }
    }
}

/// One `data` entry of a NEP-141 `ft_transfer` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtTransferEvent {
    pub old_owner_id: String,
    pub new_owner_id: String,
    pub amount: String,
    pub memo: Option<String>,
}

/// Parse every NEP-141 `ft_transfer` event entry in a list of log lines.
/// Lines that are not `EVENT_JSON`, not `nep141`, not `ft_transfer`, or
/// not parseable are skipped. `data` is an array: one line may carry
/// several transfers.
pub fn ft_transfer_events(logs: &[String]) -> Vec<FtTransferEvent> {
    let mut events = Vec::new();
    for line in logs {
        let Some(body) = line.strip_prefix(EVENT_PREFIX) else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<Value>(body.trim()) else {
            continue;
        };
        if event.get("standard").and_then(Value::as_str) != Some("nep141")
            || event.get("event").and_then(Value::as_str) != Some("ft_transfer")
        {
            continue;
        }
        let Some(data) = event.get("data").and_then(Value::as_array) else {
            continue;
        };
        for entry in data {
            let field = |name: &str| entry.get(name).and_then(Value::as_str).map(str::to_string);
            let (Some(old_owner_id), Some(new_owner_id), Some(amount)) = (
                field("old_owner_id"),
                field("new_owner_id"),
                field("amount"),
            ) else {
                continue;
            };
            events.push(FtTransferEvent {
                old_owner_id,
                new_owner_id,
                amount,
                memo: field("memo"),
            });
        }
    }
    events
}

/// Receipt outcomes reachable from the transaction, in depth-first
/// pre-order from the transaction's root receipts (children in
/// `receipt_ids` order). Falls back to the node's order when the root
/// list is absent.
fn reachable_receipts(outcome: &TxOutcome) -> Vec<&ReceiptOutcome> {
    let by_id: HashMap<&str, &ReceiptOutcome> = outcome
        .receipts_outcome
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    if outcome.root_receipt_ids.is_empty() {
        return outcome.receipts_outcome.iter().collect();
    }
    let mut ordered = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = outcome
        .root_receipt_ids
        .iter()
        .rev()
        .map(String::as_str)
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(receipt) = by_id.get(id) else {
            continue;
        };
        ordered.push(*receipt);
        for child in receipt.receipt_ids.iter().rev() {
            stack.push(child.as_str());
        }
    }
    ordered
}

#[derive(Default)]
struct Scan {
    asset_seen: bool,
    asset_failure: Option<String>,
    saw_transfer_call: bool,
    saw_ft_transfer: bool,
    bad_deposit: bool,
    bad_amount: bool,
    ambiguous: bool,
    event_to_gateway: bool,
    candidates: Vec<TransferEvidence>,
    orphans: Vec<OrphanTransfer>,
}

fn function_calls(actions: &[TxAction]) -> impl Iterator<Item = (&str, &str)> {
    actions.iter().filter_map(|action| match action {
        TxAction::FunctionCall {
            method_name,
            deposit_yocto,
            ..
        } => Some((method_name.as_str(), deposit_yocto.as_str())),
        TxAction::Other => None,
    })
}

fn scan_receipts(intent: &DepositIntent, outcome: &TxOutcome, bodies: &[ReceiptView]) -> Scan {
    let asset = intent.asset_id.as_str();
    let gateway = intent.deposit_address.as_str();
    let memo = intent.intent_id.as_str();
    let body_by_id: HashMap<&str, &ReceiptView> =
        bodies.iter().map(|b| (b.receipt_id.as_str(), b)).collect();
    let mut scan = Scan::default();
    for receipt in reachable_receipts(outcome) {
        if receipt.executor_id != asset {
            // Spoof resistance: logs from any other executor are ignored.
            continue;
        }
        scan.asset_seen = true;
        if let OutcomeStatus::Failure(reason) = &receipt.status {
            scan.asset_failure.get_or_insert_with(|| reason.clone());
            continue;
        }
        if !receipt.status.is_success() {
            continue;
        }
        let Some(actions) = body_by_id
            .get(receipt.id.as_str())
            .and_then(|b| b.actions.as_ref())
        else {
            continue;
        };
        let calls: Vec<(&str, &str)> = function_calls(actions).collect();
        if calls.iter().any(|(m, _)| *m == "ft_transfer_call") {
            scan.saw_transfer_call = true;
            continue;
        }
        if !calls.iter().any(|(m, _)| *m == "ft_transfer") {
            continue;
        }
        scan.saw_ft_transfer = true;
        if !calls.iter().any(|(m, d)| *m == "ft_transfer" && *d == "1") {
            scan.bad_deposit = true;
            continue;
        }
        let events = ft_transfer_events(&receipt.logs);
        if events.iter().any(|e| e.new_owner_id == gateway) {
            scan.event_to_gateway = true;
        }
        let matching: Vec<&FtTransferEvent> = events
            .iter()
            .filter(|e| e.new_owner_id == gateway && e.memo.as_deref() == Some(memo))
            .collect();
        let predecessor = body_by_id
            .get(receipt.id.as_str())
            .map(|b| b.predecessor_id.clone())
            .unwrap_or_default();
        match matching.as_slice() {
            [] => {}
            [event] => {
                if event.amount.parse::<u128>().is_err() {
                    scan.bad_amount = true;
                    continue;
                }
                scan.candidates.push(TransferEvidence {
                    tx_hash: outcome.tx_hash.clone(),
                    receipt_id: receipt.id.clone(),
                    executor_id: receipt.executor_id.clone(),
                    predecessor_id: predecessor,
                    payer_id: event.old_owner_id.clone(),
                    receiver_id: event.new_owner_id.clone(),
                    amount_atomic: event.amount.clone(),
                    memo: memo.to_string(),
                });
            }
            several => {
                // One token receipt cannot be mapped to one ft_transfer:
                // none of its events funds; all are orphans.
                scan.ambiguous = true;
                for event in several {
                    scan.orphans.push(OrphanTransfer {
                        tx_hash: outcome.tx_hash.clone(),
                        receipt_id: receipt.id.clone(),
                        payer_id: event.old_owner_id.clone(),
                        amount_atomic: event.amount.clone(),
                        reason: format!(
                            "one token receipt carries {} matching transfer events",
                            several.len()
                        ),
                    });
                }
            }
        }
    }
    scan
}

fn orphan_of(evidence: &TransferEvidence, reason: &str) -> OrphanTransfer {
    OrphanTransfer {
        tx_hash: evidence.tx_hash.clone(),
        receipt_id: evidence.receipt_id.clone(),
        payer_id: evidence.payer_id.clone(),
        amount_atomic: evidence.amount_atomic.clone(),
        reason: reason.to_string(),
    }
}

/// Verify a fetched transaction outcome against an intent. Pure: the same
/// outcome always yields the same check, so recorded fixtures exercise
/// exactly the live decision path.
pub fn check_outcome(intent: &DepositIntent, outcome: &TxOutcome) -> ReceiptCheck {
    if let Some(level) = &outcome.final_execution_status {
        if level != "FINAL" {
            return ReceiptCheck::only(Verification::NotFinalYet);
        }
    }
    let top_failure = match &outcome.status {
        TxStatus::Unknown => return ReceiptCheck::only(Verification::NotFinalYet),
        TxStatus::Malformed(message) => {
            return ReceiptCheck::only(Verification::StructuralMismatch(format!(
                "malformed transaction status: {message}"
            )))
        }
        TxStatus::Failed(reason) => Some(reason.clone()),
        TxStatus::Success => None,
    };
    let Some(bodies) = &outcome.receipts else {
        return ReceiptCheck::only(Verification::StructuralMismatch(
            "the transaction status carries no receipt bodies; the receipt method cannot be bound"
                .to_string(),
        ));
    };
    let mut scan = scan_receipts(intent, outcome, bodies);
    let mut orphans = std::mem::take(&mut scan.orphans);

    if let Some(reason) = top_failure {
        // The payment is not confirmed. A token transfer that still
        // landed (for example before a failing callback) is an orphan.
        for candidate in &scan.candidates {
            orphans.push(orphan_of(
                candidate,
                "the transaction's final status is Failure",
            ));
        }
        return ReceiptCheck {
            verification: Verification::Failed(reason),
            evidence: None,
            orphans,
        };
    }

    if let Some((first, rest)) = scan.candidates.split_first() {
        for extra in rest {
            orphans.push(orphan_of(
                extra,
                "a second matching transfer for an intent that is already funded",
            ));
        }
        return ReceiptCheck {
            verification: Verification::Confirmed {
                tx_ref: outcome.tx_hash.clone(),
                amount_atomic: first.amount_atomic.clone(),
            },
            evidence: Some(first.clone()),
            orphans,
        };
    }

    if let Some(reason) = scan.asset_failure {
        return ReceiptCheck {
            verification: Verification::Failed(format!(
                "the token transfer receipt failed: {reason}"
            )),
            evidence: None,
            orphans,
        };
    }

    let message = if !scan.asset_seen {
        format!(
            "no receipt in the transaction was executed by the settlement asset contract {}; \
             logs from other contracts are ignored",
            intent.asset_id
        )
    } else if scan.saw_transfer_call && !scan.saw_ft_transfer {
        "the payment used ft_transfer_call, which the token may refund; pay with ft_transfer"
            .to_string()
    } else if scan.bad_deposit {
        "ft_transfer must attach exactly 1 yoctoNEAR (NEP-141)".to_string()
    } else if !scan.saw_ft_transfer {
        "the transaction contains no ft_transfer function call on the settlement asset".to_string()
    } else if scan.bad_amount {
        "the matching ft_transfer event amount is not an integer".to_string()
    } else if scan.ambiguous {
        "a token receipt carries more than one matching transfer event; none funds the intent"
            .to_string()
    } else if !scan.event_to_gateway {
        "no nep141 ft_transfer event from the settlement asset pays the gateway account".to_string()
    } else {
        "no nep141 ft_transfer event to the gateway account carries this intent memo".to_string()
    };
    ReceiptCheck {
        verification: Verification::StructuralMismatch(message),
        evidence: None,
        orphans,
    }
}

/// Fetch a hinted transaction with its receipt tree and check it against
/// an intent. Blocking (one or two RPC round-trips): the API layer runs
/// it on the blocking thread pool.
///
/// Not found and timeout answers map to `NotFinalYet`; any other RPC
/// failure is `ProviderUnavailable`.
pub fn verify_tx_receipts(
    rpc: NearRpc,
    intent: &DepositIntent,
    hint: &TxHint,
) -> Result<ReceiptCheck, ProviderError> {
    match rpc.tx_status_receipts(&hint.tx_hash, &hint.signer_id) {
        Ok(outcome) => Ok(check_outcome(intent, &outcome)),
        Err(err) if err.is_not_found() || err.is_timeout() => {
            Ok(ReceiptCheck::only(Verification::NotFinalYet))
        }
        Err(_) => Err(ProviderError::ProviderUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_parsing_skips_foreign_and_malformed_lines() {
        let logs = vec![
            "plain log".to_string(),
            "EVENT_JSON:not json".to_string(),
            r#"EVENT_JSON:{"standard":"nep171","version":"1.0.0","event":"nft_transfer","data":[]}"#
                .to_string(),
            r#"EVENT_JSON:{"standard":"nep141","version":"1.0.0","event":"ft_mint","data":[{"owner_id":"a","amount":"1"}]}"#
                .to_string(),
            r#"EVENT_JSON: {"standard":"nep141","version":"1.0.0","event":"ft_transfer","data":[{"old_owner_id":"a","new_owner_id":"b","amount":"5","memo":"m"},{"old_owner_id":"a","new_owner_id":"c","amount":"6"},{"new_owner_id":"d"}]}"#
                .to_string(),
        ];
        let events = ft_transfer_events(&logs);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].new_owner_id, "b");
        assert_eq!(events[0].memo.as_deref(), Some("m"));
        assert_eq!(events[1].memo, None);
    }
}
