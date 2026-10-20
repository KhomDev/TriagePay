//! JSON DTOs crossing the wire boundary.
//!
//! Money rule: every `Amount` serializes as an object carrying both the
//! atomic units (a decimal string of u128 atomic units, safe for
//! JavaScript consumers) and a display-only decimal string formatted at
//! the settlement width. Atomic units are the source of truth; the
//! decimal rendering is never parsed back.
//!
//! Request bodies are strict: `serde(deny_unknown_fields)` plus explicit
//! string types for money, so a JSON number where a string is expected is
//! a 400, not a silent coercion.

use aps_core::{
    Amount, Balances, DepositReceipt, JobSnapshot, RefundReceipt, Reservation, Settlement,
    SETTLEMENT_DECIMALS,
};
use serde::{Deserialize, Serialize};

/// One money value on the wire: atomic units plus display decimal.
#[derive(Debug, Serialize)]
pub struct AmountJson {
    pub atomic: String,
    pub decimal: String,
}

impl From<Amount> for AmountJson {
    fn from(amount: Amount) -> Self {
        Self {
            atomic: amount.atomic().to_string(),
            decimal: amount.format(SETTLEMENT_DECIMALS),
        }
    }
}

/// Balance snapshot of one account.
#[derive(Debug, Serialize)]
pub struct BalancesJson {
    pub available: AmountJson,
    pub reserved: AmountJson,
    pub settled_total: AmountJson,
    pub refunded_total: AmountJson,
    pub deposited_total: AmountJson,
}

impl From<Balances> for BalancesJson {
    fn from(balances: Balances) -> Self {
        Self {
            available: balances.available.into(),
            reserved: balances.reserved.into(),
            settled_total: balances.settled_total.into(),
            refunded_total: balances.refunded_total.into(),
            deposited_total: balances.deposited_total.into(),
        }
    }
}

/// `GET /health` response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

/// `POST /accounts/{account_id}/deposits` request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositRequest {
    /// Atomic units of credit to add, as a decimal string.
    pub amount_atomic: String,
    /// Caller-scoped idempotency key.
    pub idempotency_key: String,
}

/// `POST /accounts/{account_id}/deposits` receipt.
#[derive(Debug, Serialize)]
pub struct DepositReceiptJson {
    pub account: String,
    pub credited: AmountJson,
    pub replayed: bool,
    pub balances: BalancesJson,
}

impl From<DepositReceipt> for DepositReceiptJson {
    fn from(receipt: DepositReceipt) -> Self {
        Self {
            account: receipt.account,
            credited: receipt.credited.into(),
            replayed: receipt.replayed,
            balances: receipt.balances.into(),
        }
    }
}

/// Parse a wire amount: a non-negative u128 in atomic units, encoded as
/// a JSON string. Anything else (fractions, signs, whitespace, numbers)
/// is a 400 validation error before it can reach the ledger.
pub(crate) fn parse_amount_atomic(raw: &str) -> Result<Amount, crate::error::ApiError> {
    let units: u128 = raw.parse().map_err(|_| {
        crate::error::ApiError::validation(format!(
            "amount_atomic must be a decimal string of non-negative integer atomic units, got {raw:?}"
        ))
    })?;
    Ok(Amount::from_atomic(units))
}

/// `POST /reservations` request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationRequest {
    /// Account whose available credit is locked.
    pub account_id: String,
    /// What the credit pays for, e.g. a scan SKU from the price book.
    pub sku: String,
    /// Atomic units to lock, as a decimal string.
    pub amount_atomic: String,
    /// Caller-scoped idempotency key.
    pub idempotency_key: String,
}

/// `POST /reservations` receipt.
#[derive(Debug, Serialize)]
pub struct ReservationJson {
    pub job_id: String,
    pub account: String,
    pub sku: String,
    pub reserved: AmountJson,
    pub replayed: bool,
    pub balances: BalancesJson,
}

impl From<Reservation> for ReservationJson {
    fn from(reservation: Reservation) -> Self {
        Self {
            job_id: reservation.job_id.to_string(),
            account: reservation.account,
            sku: reservation.sku,
            reserved: reservation.reserved.into(),
            replayed: reservation.replayed,
            balances: reservation.balances.into(),
        }
    }
}

/// `POST /jobs/{job_id}/settlements` request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementRequest {
    /// 64 hex characters binding the settlement to its report content.
    pub report_hash_hex: String,
}

/// `POST /jobs/{job_id}/settlements` receipt.
#[derive(Debug, Serialize)]
pub struct SettlementJson {
    pub job_id: String,
    pub charged: AmountJson,
    pub report_hash_hex: String,
    pub replayed: bool,
    pub balances: BalancesJson,
}

impl From<Settlement> for SettlementJson {
    fn from(settlement: Settlement) -> Self {
        Self {
            job_id: settlement.job_id.to_string(),
            charged: settlement.charged.into(),
            report_hash_hex: settlement.report_hash.to_hex(),
            replayed: settlement.replayed,
            balances: settlement.balances.into(),
        }
    }
}

/// `POST /jobs/{job_id}/refunds` request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefundRequest {
    /// Why the reserved credit is being returned.
    pub reason: String,
}

/// `POST /jobs/{job_id}/refunds` receipt.
#[derive(Debug, Serialize)]
pub struct RefundReceiptJson {
    pub job_id: String,
    pub refunded: AmountJson,
    pub reason: String,
    pub replayed: bool,
    pub balances: BalancesJson,
}

impl From<RefundReceipt> for RefundReceiptJson {
    fn from(receipt: RefundReceipt) -> Self {
        Self {
            job_id: receipt.job_id.to_string(),
            refunded: receipt.refunded.into(),
            reason: receipt.reason,
            replayed: receipt.replayed,
            balances: receipt.balances.into(),
        }
    }
}

/// `GET /jobs/{job_id}` snapshot.
#[derive(Debug, Serialize)]
pub struct JobSnapshotJson {
    pub job_id: String,
    pub account: String,
    pub sku: String,
    pub amount: AmountJson,
    pub state: String,
    pub settlement: Option<SettlementJson>,
    pub refund: Option<RefundReceiptJson>,
}

impl From<JobSnapshot> for JobSnapshotJson {
    fn from(snapshot: JobSnapshot) -> Self {
        Self {
            job_id: snapshot.job_id.to_string(),
            account: snapshot.account,
            sku: snapshot.sku,
            amount: snapshot.amount.into(),
            state: snapshot.state.to_string(),
            settlement: snapshot.settlement.map(Into::into),
            refund: snapshot.refund.map(Into::into),
        }
    }
}

/// `POST /jobs/{job_id}/start` response.
#[derive(Debug, Serialize)]
pub struct JobStateJson {
    pub job_id: String,
    pub state: String,
}
