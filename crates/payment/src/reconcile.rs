//! Pure reconciliation: decide whether an observation funds an intent.
//!
//! This is the anti-spoofing core of the gateway. Credit is credited
//! only when a provider-observed confirmation matches the intent on
//! amount and asset. The caller (the gateway) is responsible for
//! applying each Fund outcome at most once per intent id.

use crate::types::{AssetId, DepositIntent, Observation};
use aps_core::Amount;

/// The result of reconciling an observation against an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// Nothing final yet; poll again later.
    StillPending,
    /// The observation matches the intent exactly: fund this amount,
    /// keyed by intent id, exactly once.
    Fund { tx_ref: String },
    /// The provider confirmed a payment that does not match the intent.
    /// Never fund; surface the mismatch.
    Mismatch {
        expected_amount: Amount,
        observed_amount: Amount,
        expected_asset: AssetId,
        observed_asset: AssetId,
    },
    /// The intent expired unconfirmed.
    Expired,
    /// The provider rejected the intent.
    Rejected { reason: String },
}

/// Reconcile one observation against its intent.
pub fn reconcile(intent: &DepositIntent, observation: &Observation) -> ReconcileOutcome {
    match observation {
        Observation::Pending => ReconcileOutcome::StillPending,
        Observation::Expired => ReconcileOutcome::Expired,
        Observation::Rejected { reason } => ReconcileOutcome::Rejected {
            reason: reason.clone(),
        },
        Observation::Confirmed {
            tx_ref,
            asset_id,
            amount,
        } => {
            if *asset_id != intent.asset_id || *amount != intent.amount {
                return ReconcileOutcome::Mismatch {
                    expected_amount: intent.amount,
                    observed_amount: *amount,
                    expected_asset: intent.asset_id.clone(),
                    observed_asset: asset_id.clone(),
                };
            }
            ReconcileOutcome::Fund {
                tx_ref: tx_ref.clone(),
            }
        }
    }
}
