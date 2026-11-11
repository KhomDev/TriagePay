//! The NEAR provider: memo-bound deposit intents, verified on-chain.
//!
//! Funding model (see the crate docs and docs/integrations/near.md):
//! the payer moves NEP-141 tokens to the gateway account with
//! ft_transfer, with the intent id as the transfer memo and exactly 1
//! yoctoNEAR attached - either directly from their own account or through
//! a policy-wallet contract's `pay`. The caller then submits the
//! transaction hash (plus the signer id needed to look it up) as a hint.
//! Observation re-reads the transaction and its receipt tree from the
//! chain with wait_until "FINAL" and verifies the token contract's own
//! transfer event (see the verify module) before producing an
//! observation; the pure reconciliation in aps_payment decides funding,
//! and the idempotent ledger applies it at most once per intent.

use std::collections::HashMap;
use std::time::Duration;

use aps_payment::{
    AssetId, CreateIntent, DepositIntent, IntentId, IntentStatus, Observation, PaymentProvider,
    ProviderError, RefundRequest,
};

use crate::rpc::NearRpc;
use crate::verify::{verify_tx_receipts, OrphanTransfer, ReceiptCheck, TransferEvidence};

/// Environment variables understood by NearConfig::from_env.
pub const ENV_PROVIDER: &str = "APS_PROVIDER";
/// Selects the provider: "mock" (default) or "near".
pub const ENV_RPC_URL: &str = "NEAR_RPC_URL";
/// Network: "mainnet" or "testnet".
pub const ENV_NETWORK: &str = "NEAR_NETWORK";
/// The gateway NEAR account id (where deposits land).
pub const ENV_GATEWAY_ACCOUNT: &str = "NEAR_GATEWAY_ACCOUNT";
/// The NEP-141 contract id of the settlement asset (for example a USDC
/// contract). Used as the asset_id of every intent.
pub const ENV_ASSET_CONTRACT: &str = "NEAR_ASSET_CONTRACT";
/// Per-call RPC timeout in seconds.
pub const ENV_RPC_TIMEOUT_SECS: &str = "NEAR_RPC_TIMEOUT_SECS";

/// Configuration for the NEAR adapter. Every value is a public on-chain
/// identifier or a public endpoint; keys are never part of this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearConfig {
    pub rpc_url: String,
    pub network: String,
    pub gateway_account_id: String,
    pub asset_contract_id: String,
    pub timeout_secs: u64,
}

impl NearConfig {
    /// The default public RPC endpoint for a network (FASTNEAR's free
    /// tier). The official `rpc.{mainnet,testnet}.near.org` endpoints are
    /// deprecated and answer every request with HTTP 429 (verified
    /// 2026-11-06); `NEAR_RPC_URL` overrides this default.
    pub fn default_rpc_url(network: &str) -> &'static str {
        match network {
            "testnet" => "https://test.rpc.fastnear.com",
            _ => "https://free.rpc.fastnear.com",
        }
    }

    /// Read the configuration from the environment.
    ///
    /// Returns Ok(None) when APS_PROVIDER is unset or "mock" (the mock
    /// provider stays the default). Returns an error naming the missing
    /// variables - names only, never values - when APS_PROVIDER=near is
    /// requested but the configuration is incomplete. When NEAR_RPC_URL
    /// is absent, the official public endpoint for the selected network
    /// is used.
    pub fn from_env() -> Result<Option<NearConfig>, String> {
        let provider = std::env::var(ENV_PROVIDER).unwrap_or_default();
        let provider = provider.trim();
        if provider.is_empty() || provider.eq_ignore_ascii_case("mock") {
            return Ok(None);
        }
        if !provider.eq_ignore_ascii_case("near") {
            return Err(format!(
                "{ENV_PROVIDER} must be one of \"mock\" or \"near\", got an unrecognized value"
            ));
        }
        let mut missing: Vec<&str> = Vec::new();
        for name in [ENV_NETWORK, ENV_GATEWAY_ACCOUNT, ENV_ASSET_CONTRACT] {
            match std::env::var(name) {
                Ok(value) if !value.trim().is_empty() => {}
                _ => missing.push(name),
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "the near provider requires these environment variables to be set: {}",
                missing.join(", ")
            ));
        }
        let network = std::env::var(ENV_NETWORK)
            .unwrap_or_default()
            .trim()
            .to_string();
        if network != "mainnet" && network != "testnet" {
            return Err(format!("{ENV_NETWORK} must be \"mainnet\" or \"testnet\""));
        }
        let gateway_account_id = std::env::var(ENV_GATEWAY_ACCOUNT)
            .unwrap_or_default()
            .trim()
            .to_string();
        let asset_contract_id = std::env::var(ENV_ASSET_CONTRACT)
            .unwrap_or_default()
            .trim()
            .to_string();
        let rpc_url = match std::env::var(ENV_RPC_URL) {
            Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => Self::default_rpc_url(&network).to_string(),
        };
        let timeout_secs = std::env::var(ENV_RPC_TIMEOUT_SECS)
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(15);
        Ok(Some(NearConfig {
            rpc_url,
            network,
            gateway_account_id,
            asset_contract_id,
            timeout_secs,
        }))
    }
}

/// A client-supplied transaction hint: the hash to look up and the signer
/// id the tx RPC method requires. Hints locate a transaction; they prove
/// nothing - every material fact is re-read from the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxHint {
    pub signer_id: String,
    pub tx_hash: String,
}

/// What an on-chain verification produced, before it is applied to the
/// intent state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// The transaction is not visible or not final yet: poll again later.
    NotFinalYet,
    /// The transaction executed and failed: the payment did not happen.
    Failed(String),
    /// The transaction does not structurally match this intent (no
    /// token receipt, wrong method, recipient, memo, or attached deposit).
    /// A client error: no observation is recorded and no funds move.
    StructuralMismatch(String),
    /// A finalized token receipt whose own nep141 ft_transfer event pays
    /// the gateway account with this intent memo. amount_atomic is what
    /// the event says moved - the pure reconciliation compares it to the
    /// intent amount.
    Confirmed {
        tx_ref: String,
        amount_atomic: String,
    },
}

/// What NearProvider::prepare_fetch asks the caller to do next.
///
/// The API layer runs verify_tx on the blocking thread pool between the
/// two calls, so the async runtime is never blocked by HTTP.
#[derive(Debug, Clone)]
pub enum FetchStep {
    /// The observation is already final; return it directly.
    Ready(Observation),
    /// Verify the hinted transaction, then call
    /// NearProvider::apply_verification.
    Verify {
        rpc: NearRpc,
        intent: DepositIntent,
        hint: TxHint,
    },
}

#[derive(Debug, Clone)]
struct NearIntent {
    intent: DepositIntent,
    hint: Option<TxHint>,
    observation: Option<Observation>,
    refunded: bool,
    /// The token receipt behind a confirmation.
    evidence: Option<TransferEvidence>,
    /// Matching transfers that did not fund this intent.
    orphans: Vec<OrphanTransfer>,
}

impl NearIntent {
    fn add_orphans(&mut self, orphans: impl IntoIterator<Item = OrphanTransfer>) {
        for orphan in orphans {
            let known = self
                .orphans
                .iter()
                .any(|o| o.tx_hash == orphan.tx_hash && o.receipt_id == orphan.receipt_id);
            if !known {
                self.orphans.push(orphan);
            }
        }
    }
}

/// The NEAR payment provider: deterministic, verification-only, and
/// replay-safe. Observations are idempotent - once an intent has a final
/// observation, polling it again returns the same value forever.
#[derive(Debug)]
pub struct NearProvider {
    rpc: NearRpc,
    config: NearConfig,
    intents: HashMap<IntentId, NearIntent>,
    next_intent: u64,
    next_refund: u64,
    unix_now: u64,
}

impl NearProvider {
    /// Construct the provider with an explicit configuration.
    pub fn new(config: NearConfig) -> Result<Self, ProviderError> {
        let timeout = Duration::from_secs(config.timeout_secs.max(1));
        let rpc = NearRpc::new(&config.rpc_url, timeout)
            .map_err(|err| ProviderError::Validation(err.describe()))?;
        Ok(Self {
            rpc,
            config,
            intents: HashMap::new(),
            next_intent: 0,
            next_refund: 0,
            unix_now: 0,
        })
    }

    /// The configuration in use (public values only).
    pub fn config(&self) -> &NearConfig {
        &self.config
    }

    /// Read-only view of an intent, if present.
    pub fn intent(&self, intent_id: &IntentId) -> Option<&DepositIntent> {
        self.intents.get(intent_id).map(|entry| &entry.intent)
    }

    /// The token-receipt evidence behind an intent's confirmation.
    pub fn evidence(&self, intent_id: &IntentId) -> Option<&TransferEvidence> {
        self.intents
            .get(intent_id)
            .and_then(|entry| entry.evidence.as_ref())
    }

    /// Matching transfers recorded against an intent that did not fund
    /// it (refundable only by an operator-approved transfer).
    pub fn orphans(&self, intent_id: &IntentId) -> &[OrphanTransfer] {
        self.intents
            .get(intent_id)
            .map(|entry| entry.orphans.as_slice())
            .unwrap_or(&[])
    }

    /// A handle on the RPC client, for read-only checks that do not
    /// belong to an intent (the x402 facilitator /verify).
    pub fn rpc(&self) -> NearRpc {
        self.rpc.clone()
    }

    /// Everything needed to check a hint that is NOT the intent's own
    /// (for example a second payment for an already-funded intent)
    /// without storing it. Pair with [`NearProvider::apply_check`].
    pub fn prepare_probe(
        &self,
        intent_id: &IntentId,
        hint: TxHint,
    ) -> Result<FetchStep, ProviderError> {
        let entry = self
            .intents
            .get(intent_id)
            .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
        Ok(FetchStep::Verify {
            rpc: self.rpc.clone(),
            intent: entry.intent.clone(),
            hint,
        })
    }

    /// Apply a completed receipt-level check: record its evidence and
    /// orphans, then apply its verification.
    ///
    /// When the intent already has a final observation, nothing about
    /// the funding changes: a confirmation from a different transaction
    /// is recorded as an orphan, never as extra credit. A confirmed
    /// amount that differs from the intent is also recorded as an
    /// orphan (exact-amount semantics; the reconciliation never funds it)
    /// and is not kept as the intent's evidence: the intent stays open
    /// for the exact payment (see [`NearProvider::apply_verification`]).
    pub fn apply_check(
        &mut self,
        intent_id: &IntentId,
        check: ReceiptCheck,
    ) -> Result<Observation, ProviderError> {
        let entry = self
            .intents
            .get_mut(intent_id)
            .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
        entry.add_orphans(check.orphans);
        if let Some(observation) = entry.observation.clone() {
            if let Some(evidence) = check.evidence {
                let same = entry
                    .evidence
                    .as_ref()
                    .is_some_and(|known| known.tx_hash == evidence.tx_hash);
                if !same {
                    entry.add_orphans([OrphanTransfer {
                        tx_hash: evidence.tx_hash,
                        receipt_id: evidence.receipt_id,
                        payer_id: evidence.payer_id,
                        amount_atomic: evidence.amount_atomic,
                        reason:
                            "the intent already has a final observation from another transaction"
                                .to_string(),
                    }]);
                }
            }
            return Ok(observation);
        }
        if let Some(evidence) = &check.evidence {
            let exact =
                evidence.amount_atomic.parse::<u128>().ok() == Some(entry.intent.amount.atomic());
            if exact {
                entry.evidence = Some(evidence.clone());
            } else {
                entry.add_orphans([OrphanTransfer {
                    tx_hash: evidence.tx_hash.clone(),
                    receipt_id: evidence.receipt_id.clone(),
                    payer_id: evidence.payer_id.clone(),
                    amount_atomic: evidence.amount_atomic.clone(),
                    reason: "the transferred amount does not match the intent".to_string(),
                }]);
            }
        }
        self.apply_verification(intent_id, check.verification)
    }

    /// Advance the provider clock; never moves backwards. The gateway
    /// calls this before each provider operation so expiry decisions
    /// track wall time without sleeps.
    pub fn sync_clock(&mut self, now: u64) {
        if now > self.unix_now {
            self.unix_now = now;
        }
    }

    /// Store the client hint for an intent. The latest hint wins until
    /// the intent has a final observation; after that, a new hint is
    /// rejected - a final observation cannot be changed.
    pub fn submit_hint(&mut self, intent_id: &IntentId, hint: TxHint) -> Result<(), ProviderError> {
        let entry = self
            .intents
            .get_mut(intent_id)
            .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
        if entry.observation.is_some() {
            // Re-submitting the SAME hint is an idempotent replay (the
            // final observation is returned unchanged); a DIFFERENT hint
            // cannot change a final observation.
            if entry.hint.as_ref() == Some(&hint) {
                return Ok(());
            }
            return Err(ProviderError::Validation(
                "the intent already has a final observation; a different hint cannot change it"
                    .to_string(),
            ));
        }
        entry.hint = Some(hint);
        Ok(())
    }

    /// First half of an observation: decide whether chain data is needed
    /// and, if so, hand out everything the blocking verifier needs.
    pub fn prepare_fetch(&mut self, intent_id: &IntentId) -> Result<FetchStep, ProviderError> {
        let entry = self
            .intents
            .get_mut(intent_id)
            .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
        if let Some(observation) = &entry.observation {
            return Ok(FetchStep::Ready(observation.clone()));
        }
        if self.unix_now >= entry.intent.expires_at_unix {
            entry.intent.status = IntentStatus::Expired;
            entry.observation = Some(Observation::Expired);
            return Ok(FetchStep::Ready(Observation::Expired));
        }
        let hint = entry.hint.clone().ok_or_else(|| {
            ProviderError::Validation(
                "the near provider requires a transaction hint (signer_id and tx_hash)                  before it can observe an intent"
                    .to_string(),
            )
        })?;
        Ok(FetchStep::Verify {
            rpc: self.rpc.clone(),
            intent: entry.intent.clone(),
            hint,
        })
    }

    /// Second half of an observation: apply a completed verification.
    /// Idempotent - if the intent already has a final observation, it is
    /// returned unchanged.
    ///
    /// Only an exact-amount confirmation is final. Two results are
    /// reported for the hinted transaction and leave the intent open, so
    /// that a hint anyone can submit never decides the intent's fate:
    ///
    /// - a failed transaction (`Rejected`): nothing binds a failed
    ///   transaction to this intent, so any failed transaction on chain
    ///   could otherwise be used to close a requirement someone else is
    ///   paying;
    /// - a confirmed transfer of the wrong amount (`Confirmed`, which
    ///   reconciles to a mismatch): a one-unit transfer carrying the memo
    ///   could otherwise block the exact payment.
    ///
    /// In both cases a later hint for the real payment still funds the
    /// intent, until it expires.
    pub fn apply_verification(
        &mut self,
        intent_id: &IntentId,
        verification: Verification,
    ) -> Result<Observation, ProviderError> {
        let entry = self
            .intents
            .get_mut(intent_id)
            .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
        if let Some(observation) = &entry.observation {
            return Ok(observation.clone());
        }
        match verification {
            Verification::NotFinalYet => Ok(Observation::Pending),
            // Not final: reported for this hint only (see the method docs).
            Verification::Failed(reason) => Ok(Observation::Rejected { reason }),
            Verification::StructuralMismatch(message) => Err(ProviderError::Validation(message)),
            Verification::Confirmed {
                tx_ref,
                amount_atomic,
            } => {
                let units: u128 = amount_atomic.parse().map_err(|_| {
                    ProviderError::Validation("observed amount is not an integer".to_string())
                })?;
                let amount = aps_core::Amount::from_atomic(units);
                let observation = Observation::Confirmed {
                    tx_ref: tx_ref.clone(),
                    asset_id: AssetId::new(entry.intent.asset_id.as_str()),
                    amount,
                };
                if amount != entry.intent.amount {
                    // Not final: the reconciliation reports the mismatch
                    // and the intent stays open for the exact payment.
                    return Ok(observation);
                }
                entry.intent.status = IntentStatus::Confirmed { tx_ref };
                entry.observation = Some(observation.clone());
                Ok(observation)
            }
        }
    }
}

/// Verify a hinted transaction against an intent at the receipt level.
/// Blocking (one or two RPC round-trips): the API layer runs it on the
/// blocking thread pool.
///
/// Every fact is read from the chain: the final status, the receipt
/// tree, the token receipt's executor, own status, method and attached
/// deposit, and the token contract's own nep141 transfer event
/// (recipient, memo, amount). The hint itself only locates the
/// transaction. See [`crate::verify`] for the rules and
/// [`crate::verify::verify_tx_receipts`] for the evidence and orphans.
pub fn verify_tx(
    rpc: NearRpc,
    intent: &DepositIntent,
    hint: &TxHint,
) -> Result<Verification, ProviderError> {
    Ok(verify_tx_receipts(rpc, intent, hint)?.verification)
}

impl PaymentProvider for NearProvider {
    fn create_intent(&mut self, req: CreateIntent) -> Result<DepositIntent, ProviderError> {
        if self.unix_now == 0 {
            return Err(ProviderError::Validation(
                "the provider clock must be synced before creating intents".to_string(),
            ));
        }
        if req.amount.is_zero() {
            return Err(ProviderError::Validation(
                "intent amount must be positive".to_string(),
            ));
        }
        if req.expires_at_unix <= self.unix_now {
            return Err(ProviderError::Validation(
                "intent expiry must be in the future".to_string(),
            ));
        }
        if req.asset_id.as_str() != self.config.asset_contract_id {
            return Err(ProviderError::Validation(format!(
                "the near provider funds credit only in {} (the configured settlement asset)",
                self.config.asset_contract_id
            )));
        }
        // Intent ids double as NEAR transfer memos and as the x402 request
        // binding, so they must be unguessable as well as unique. Whoever
        // knows a memo before the payer has paid can aim a transfer at
        // it; a clock and a counter alone would let anyone compute the
        // next memo from the last one. The 128-bit nonce comes from the
        // operating system; the clock and the counter stay in the id only
        // to make it readable in logs.
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).map_err(|_| ProviderError::ProviderUnavailable)?;
        self.next_intent += 1;
        let id = IntentId::new(format!(
            "aps2_{:x}_{:05x}_{:032x}",
            self.unix_now,
            self.next_intent,
            u128::from_be_bytes(nonce)
        ));
        let intent = DepositIntent {
            intent_id: id.clone(),
            deposit_address: self.config.gateway_account_id.clone(),
            asset_id: req.asset_id,
            amount: req.amount,
            expires_at_unix: req.expires_at_unix,
            status: IntentStatus::Pending,
        };
        self.intents.insert(
            id,
            NearIntent {
                intent: intent.clone(),
                hint: None,
                observation: None,
                refunded: false,
                evidence: None,
                orphans: Vec::new(),
            },
        );
        Ok(intent)
    }

    fn observe(&mut self, intent_id: &IntentId) -> Result<Observation, ProviderError> {
        let step = self.prepare_fetch(intent_id)?;
        let check = match step {
            FetchStep::Ready(observation) => return Ok(observation),
            FetchStep::Verify { rpc, intent, hint } => verify_tx_receipts(rpc, &intent, &hint)?,
        };
        self.apply_check(intent_id, check)
    }

    fn request_refund(
        &mut self,
        intent_id: &IntentId,
        reason: &str,
    ) -> Result<RefundRequest, ProviderError> {
        let (amount, confirmed, refunded, gateway, asset) = {
            let entry = self
                .intents
                .get(intent_id)
                .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
            let confirmed = matches!(entry.intent.status, IntentStatus::Confirmed { .. })
                || matches!(entry.observation, Some(Observation::Confirmed { .. }));
            (
                entry.intent.amount,
                confirmed,
                entry.refunded,
                entry.intent.deposit_address.clone(),
                entry.intent.asset_id.clone(),
            )
        };
        if refunded {
            return Err(ProviderError::AlreadyRefunded(intent_id.to_string()));
        }
        if !confirmed {
            return Err(ProviderError::Validation(
                "cannot refund an intent that is not confirmed".to_string(),
            ));
        }
        if let Some(entry) = self.intents.get_mut(intent_id) {
            entry.refunded = true;
        }
        self.next_refund += 1;
        // Proposal only: no on-chain movement is executed here. See
        // docs/integrations/near.md - refunds require a human-approved
        // ft_transfer from the gateway account.
        Ok(RefundRequest {
            refund_id: format!("near-proposal-{:05x}", self.next_refund),
            intent_id: intent_id.clone(),
            amount,
            reason: format!(
                "PROPOSAL ONLY - no on-chain refund is executed automatically: transfer {} \
                 atomic units of {} from {} back to the payer. Operator approval required. \
                 Caller reason: {}",
                amount.atomic(),
                asset,
                gateway,
                reason
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> NearConfig {
        NearConfig {
            rpc_url: "http://127.0.0.1:9".to_string(),
            network: "mainnet".to_string(),
            gateway_account_id: "gateway.near".to_string(),
            asset_contract_id: "asset.near".to_string(),
            timeout_secs: 1,
        }
    }

    fn request(amount: u128, expires: u64) -> CreateIntent {
        CreateIntent {
            asset_id: AssetId::new("asset.near"),
            amount: aps_core::Amount::from_atomic(amount),
            expires_at_unix: expires,
        }
    }

    #[test]
    fn creation_validates_clock_amount_expiry_and_asset() {
        let mut provider = NearProvider::new(config()).unwrap();
        // Clock not synced yet.
        assert!(matches!(
            provider.create_intent(request(1_000_000, 100)),
            Err(ProviderError::Validation(_))
        ));
        provider.sync_clock(100);
        assert!(matches!(
            provider.create_intent(request(0, 200)),
            Err(ProviderError::Validation(_))
        ));
        assert!(matches!(
            provider.create_intent(request(1_000_000, 100)),
            Err(ProviderError::Validation(_))
        ));
        let other_asset = CreateIntent {
            asset_id: AssetId::new("other.near"),
            amount: aps_core::Amount::from_atomic(1_000_000),
            expires_at_unix: 200,
        };
        assert!(matches!(
            provider.create_intent(other_asset),
            Err(ProviderError::Validation(_))
        ));
        assert!(provider.create_intent(request(1_000_000, 200)).is_ok());
    }

    #[test]
    fn intent_ids_are_unique_and_addressed_to_the_gateway() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let a = provider.create_intent(request(1, 200)).unwrap();
        let b = provider.create_intent(request(1, 200)).unwrap();
        assert_ne!(a.intent_id, b.intent_id);
        assert_eq!(a.deposit_address, "gateway.near");
        assert_eq!(a.asset_id.as_str(), "asset.near");
        assert!(a.intent_id.as_str().starts_with("aps2_"));
    }

    #[test]
    fn intent_ids_end_in_a_128_bit_nonce_that_is_not_a_counter() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let nonces: Vec<u128> = (0..64)
            .map(|_| {
                let intent = provider.create_intent(request(1, 200)).unwrap();
                let (_, nonce) = intent.intent_id.as_str().rsplit_once('_').unwrap();
                assert_eq!(nonce.len(), 32, "the nonce is 32 hex characters");
                u128::from_str_radix(nonce, 16).unwrap()
            })
            .collect();
        for pair in nonces.windows(2) {
            assert_ne!(pair[0], pair[1]);
            assert_ne!(pair[1], pair[0].wrapping_add(1), "nonces must not count up");
        }
        let distinct: std::collections::HashSet<u128> = nonces.iter().copied().collect();
        assert_eq!(distinct.len(), nonces.len());
    }

    #[test]
    fn missing_hint_is_a_client_error() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1, 200)).unwrap();
        assert!(matches!(
            provider.observe(&intent.intent_id),
            Err(ProviderError::Validation(_))
        ));
    }

    #[test]
    fn expiry_yields_expired_once() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1, 200)).unwrap();
        provider.sync_clock(250);
        assert_eq!(
            provider.observe(&intent.intent_id),
            Ok(Observation::Expired)
        );
        // Replaying the expired intent is stable.
        assert_eq!(
            provider.observe(&intent.intent_id),
            Ok(Observation::Expired)
        );
        assert!(matches!(
            provider.intent(&intent.intent_id).map(|i| &i.status),
            Some(IntentStatus::Expired)
        ));
    }

    #[test]
    fn refund_requires_confirmation_and_is_one_shot_proposal() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1_000_000, 400)).unwrap();
        assert!(matches!(
            provider.request_refund(&intent.intent_id, "unused"),
            Err(ProviderError::Validation(_))
        ));
        // Force a confirmed state by applying a verification directly.
        let verification = Verification::Confirmed {
            tx_ref: "txhash".to_string(),
            amount_atomic: "1000000".to_string(),
        };
        let observation = provider
            .apply_verification(&intent.intent_id, verification)
            .unwrap();
        assert!(matches!(observation, Observation::Confirmed { .. }));
        let refund = provider
            .request_refund(&intent.intent_id, "unused")
            .expect("confirmed intents produce a refund proposal");
        assert!(refund.reason.contains("PROPOSAL ONLY"));
        assert!(refund.refund_id.starts_with("near-proposal-"));
        assert!(matches!(
            provider.request_refund(&intent.intent_id, "again"),
            Err(ProviderError::AlreadyRefunded(_))
        ));
    }

    #[test]
    fn unknown_intent_is_a_typed_error() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let id = IntentId::new("aps2_missing".to_string());
        assert!(matches!(
            provider.observe(&id),
            Err(ProviderError::UnknownIntent(_))
        ));
        assert!(matches!(
            provider.submit_hint(
                &id,
                TxHint {
                    signer_id: "s".into(),
                    tx_hash: "t".into()
                }
            ),
            Err(ProviderError::UnknownIntent(_))
        ));
    }

    #[test]
    fn apply_verification_is_idempotent() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1_000_000, 400)).unwrap();
        let verification = Verification::Confirmed {
            tx_ref: "txhash".to_string(),
            amount_atomic: "1000000".to_string(),
        };
        let first = provider
            .apply_verification(&intent.intent_id, verification.clone())
            .unwrap();
        let second = provider
            .apply_verification(&intent.intent_id, verification)
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_failed_transaction_is_reported_but_never_closes_the_intent() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1_000_000, 400)).unwrap();
        let observation = provider
            .apply_verification(
                &intent.intent_id,
                Verification::Failed("Action #0 failed".to_string()),
            )
            .unwrap();
        assert!(matches!(observation, Observation::Rejected { .. }));
        // The intent is still pending: the real payment funds it.
        assert!(matches!(
            provider.intent(&intent.intent_id).map(|i| &i.status),
            Some(IntentStatus::Pending)
        ));
        let funded = provider
            .apply_verification(
                &intent.intent_id,
                Verification::Confirmed {
                    tx_ref: "real".to_string(),
                    amount_atomic: "1000000".to_string(),
                },
            )
            .unwrap();
        assert!(matches!(funded, Observation::Confirmed { .. }));
        assert!(matches!(
            provider.intent(&intent.intent_id).map(|i| &i.status),
            Some(IntentStatus::Confirmed { .. })
        ));
    }

    #[test]
    fn a_wrong_amount_is_reported_but_never_closes_the_intent() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1_000_000, 400)).unwrap();
        let dust = provider
            .apply_verification(
                &intent.intent_id,
                Verification::Confirmed {
                    tx_ref: "dust".to_string(),
                    amount_atomic: "1".to_string(),
                },
            )
            .unwrap();
        assert!(matches!(
            aps_payment::reconcile(&intent, &dust),
            aps_payment::ReconcileOutcome::Mismatch { .. }
        ));
        assert!(matches!(
            provider.intent(&intent.intent_id).map(|i| &i.status),
            Some(IntentStatus::Pending)
        ));
        let exact = provider
            .apply_verification(
                &intent.intent_id,
                Verification::Confirmed {
                    tx_ref: "real".to_string(),
                    amount_atomic: "1000000".to_string(),
                },
            )
            .unwrap();
        assert!(matches!(
            aps_payment::reconcile(&intent, &exact),
            aps_payment::ReconcileOutcome::Fund { .. }
        ));
    }

    #[test]
    fn structural_mismatch_never_records_an_observation() {
        let mut provider = NearProvider::new(config()).unwrap();
        provider.sync_clock(100);
        let intent = provider.create_intent(request(1_000_000, 400)).unwrap();
        assert!(matches!(
            provider.apply_verification(
                &intent.intent_id,
                Verification::StructuralMismatch("memo does not match".to_string())
            ),
            Err(ProviderError::Validation(_))
        ));
        // The intent is still present and unfinalized.
        assert!(provider.intent(&intent.intent_id).is_some());
    }
}
