//! Payment-intent routes: funding credit from provider observations.
//!
//! This is the anti-spoofing surface of the gateway. A caller can create
//! a deposit intent and poll it, but credit enters an account ONLY when
//! the provider itself observes a confirmation that reconciles exactly
//! (same amount, same asset) against the intent. A client assertion of
//! "I paid" has no route into the ledger.
//!
//! The provider here is the deterministic mock from aps-payment; a real
//! adapter implements the same PaymentProvider trait behind this same
//! route contract. The mock scenario field on creation is a test and
//! demo control surface and would not exist on a real adapter.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use aps_payment::{
    reconcile, AssetId, CreateIntent, DepositIntent, IntentId, MockProvider, MockScenario,
    Observation, PaymentProvider, ProviderError, ReconcileOutcome,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::dto::{AmountJson, DepositReceiptJson};
use crate::error::ApiError;
use crate::AppState;

/// The caller identity the gateway itself acts under when funding
/// credit from a provider confirmation.
const GATEWAY_CALLER: &str = "payment-gateway";

/// POST /payment/intents request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateIntentRequest {
    /// Account credited when a matching confirmation is observed.
    pub account_id: String,
    /// Asset identifier, opaque to the gateway.
    pub asset_id: String,
    /// Amount in atomic units, as a decimal string.
    pub amount_atomic: String,
    /// How long the intent stays open, in seconds.
    pub expires_in_secs: u64,
    /// Mock-only behavior script. Absent means confirm on first poll.
    #[serde(default)]
    pub mock_scenario: Option<MockScenarioJson>,
}

/// Wire shape of a mock scenario.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MockScenarioJson {
    ConfirmOnPoll { polls: u32 },
    ConfirmWrongAmount { amount_atomic: String },
    ConfirmWrongAsset { asset_id: String },
    NeverConfirm,
    Reject { reason: String },
}

impl TryFrom<MockScenarioJson> for MockScenario {
    type Error = ApiError;

    fn try_from(value: MockScenarioJson) -> Result<Self, Self::Error> {
        match value {
            MockScenarioJson::ConfirmOnPoll { polls } => Ok(MockScenario::ConfirmOnPoll { polls }),
            MockScenarioJson::ConfirmWrongAmount { amount_atomic } => {
                let amount = crate::dto::parse_amount_atomic(&amount_atomic)?;
                Ok(MockScenario::ConfirmWrongAmount { actual: amount })
            }
            MockScenarioJson::ConfirmWrongAsset { asset_id } => {
                Ok(MockScenario::ConfirmWrongAsset {
                    asset: AssetId::new(asset_id),
                })
            }
            MockScenarioJson::NeverConfirm => Ok(MockScenario::NeverConfirm),
            MockScenarioJson::Reject { reason } => Ok(MockScenario::Reject { reason }),
        }
    }
}

/// GET /payment/intents/{intent_id} and creation response.
#[derive(Debug, serde::Serialize)]
pub struct IntentJson {
    pub intent_id: String,
    pub account_id: String,
    pub deposit_address: String,
    pub asset_id: String,
    pub amount: AmountJson,
    pub expires_at_unix: u64,
    pub status: String,
}

/// Create a deposit intent.
///
/// 201 with the intent (including the deposit address the caller pays
/// to). Credit is NOT granted here; only a provider-observed, exactly
/// matching confirmation funds the account.
pub async fn create_intent(
    State(state): State<AppState>,
    body: Result<Json<CreateIntentRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<(StatusCode, Json<IntentJson>), ApiError> {
    let Json(request) = body.map_err(crate::handlers::rejected_json)?;
    let amount = crate::dto::parse_amount_atomic(&request.amount_atomic)?;
    let scenario = match request.mock_scenario {
        None => MockScenario::ConfirmOnPoll { polls: 1 },
        Some(scenario) => MockScenario::try_from(scenario)?,
    };
    let now = unix_now();
    let req = CreateIntent {
        asset_id: AssetId::new(request.asset_id),
        amount,
        expires_at_unix: now.saturating_add(request.expires_in_secs),
    };
    let intent = {
        let mut provider = state.payment().lock().await;
        provider.sync_clock(now);
        provider
            .create_intent_with_scenario(req, scenario)
            .map_err(provider_create_error)?
    };
    state.register_intent(&intent.intent_id, &request.account_id)?;
    Ok((
        StatusCode::CREATED,
        Json(intent_json(&intent, &request.account_id)),
    ))
}

/// Fetch an intent.
pub async fn get_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
) -> Result<Json<IntentJson>, ApiError> {
    let account_id = state.intent_account(&intent_id)?;
    let intent = with_intent(&state, |provider| {
        provider.intent(&IntentId::new(intent_id.clone())).cloned()
    })
    .await?
    .ok_or_else(|| ApiError::unknown_intent(&intent_id))?;
    Ok(Json(intent_json(&intent, &account_id)))
}

/// Observe an intent: poll the provider and reconcile.
///
/// 200 with the observation, the reconciliation outcome, and — only when
/// the provider confirmed a payment matching the intent exactly — the
/// deposit receipt that funded the account. Replays are safe: the
/// funding deposit is idempotent on the intent id, so observing twice
/// never credits twice.
pub async fn observe_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let account_id = state.intent_account(&intent_id)?;
    let now = unix_now();
    let observation = {
        let mut provider = state.payment().lock().await;
        provider.sync_clock(now);
        provider
            .observe(&IntentId::new(intent_id.clone()))
            .map_err(provider_error)?
    };
    let intent = with_intent(&state, |provider| {
        provider.intent(&IntentId::new(intent_id.clone())).cloned()
    })
    .await?
    .ok_or_else(|| ApiError::unknown_intent(&intent_id))?;

    let outcome = reconcile(&intent, &observation);
    let mut body = json!({
        "intent_id": intent_id,
        "observation": observation_json(&observation),
        "reconciled": outcome_name(&outcome),
        "funded": false,
    });
    if let ReconcileOutcome::Fund { tx_ref } = &outcome {
        let mut ledger = state.write_ledger().await;
        let receipt = ledger.deposit(
            GATEWAY_CALLER,
            &account_id,
            intent.amount,
            &format!("intent:{intent_id}"),
        )?;
        body["funded"] = json!(true);
        body["tx_ref"] = json!(tx_ref);
        body["deposit"] = json!(DepositReceiptJson::from(receipt));
    }
    if let ReconcileOutcome::Mismatch {
        expected_amount,
        observed_amount,
        expected_asset,
        observed_asset,
    } = &outcome
    {
        body["mismatch"] = json!({
            "expected_amount": AmountJson::from(*expected_amount),
            "observed_amount": AmountJson::from(*observed_amount),
            "expected_asset": expected_asset.to_string(),
            "observed_asset": observed_asset.to_string(),
        });
    }
    Ok(Json(body))
}

/// POST /payment/intents/{intent_id}/refunds request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentRefundRequest {
    pub reason: String,
}

/// Request a provider refund of a confirmed intent.
pub async fn refund_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    body: Result<Json<IntentRefundRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(request) = body.map_err(crate::handlers::rejected_json)?;
    let _ = state.intent_account(&intent_id)?;
    let now = unix_now();
    let refund = {
        let mut provider = state.payment().lock().await;
        provider.sync_clock(now);
        provider
            .request_refund(&IntentId::new(intent_id.clone()), &request.reason)
            .map_err(provider_refund_error)?
    };
    Ok(Json(json!({
        "refund_id": refund.refund_id,
        "intent_id": refund.intent_id.to_string(),
        "amount": AmountJson::from(refund.amount),
        "reason": refund.reason,
    })))
}

fn intent_json(intent: &DepositIntent, account_id: &str) -> IntentJson {
    IntentJson {
        intent_id: intent.intent_id.to_string(),
        account_id: account_id.to_string(),
        deposit_address: intent.deposit_address.clone(),
        asset_id: intent.asset_id.to_string(),
        amount: AmountJson::from(intent.amount),
        expires_at_unix: intent.expires_at_unix,
        status: status_name(&intent.status),
    }
}

fn status_name(status: &aps_payment::IntentStatus) -> String {
    match status {
        aps_payment::IntentStatus::Pending => "pending".to_string(),
        aps_payment::IntentStatus::Confirmed { .. } => "confirmed".to_string(),
        aps_payment::IntentStatus::Expired => "expired".to_string(),
        aps_payment::IntentStatus::Rejected { reason } => format!("rejected: {reason}"),
    }
}

fn observation_json(observation: &Observation) -> serde_json::Value {
    match observation {
        Observation::Pending => json!({"kind": "pending"}),
        Observation::Confirmed {
            tx_ref,
            asset_id,
            amount,
        } => json!({
            "kind": "confirmed",
            "tx_ref": tx_ref,
            "asset_id": asset_id.to_string(),
            "amount": AmountJson::from(*amount),
        }),
        Observation::Expired => json!({"kind": "expired"}),
        Observation::Rejected { reason } => json!({"kind": "rejected", "reason": reason}),
    }
}

fn outcome_name(outcome: &ReconcileOutcome) -> &'static str {
    match outcome {
        ReconcileOutcome::StillPending => "still_pending",
        ReconcileOutcome::Fund { .. } => "fund",
        ReconcileOutcome::Mismatch { .. } => "mismatch",
        ReconcileOutcome::Expired => "expired",
        ReconcileOutcome::Rejected { .. } => "rejected",
    }
}

async fn with_intent<T>(
    state: &AppState,
    read: impl FnOnce(&mut MockProvider) -> Option<T>,
) -> Result<Option<T>, ApiError> {
    let now = unix_now();
    let mut provider = state.payment().lock().await;
    provider.sync_clock(now);
    Ok(read(&mut provider))
}

fn provider_create_error(err: ProviderError) -> ApiError {
    match err {
        ProviderError::Validation(msg) => ApiError::validation(msg),
        other => provider_error(other),
    }
}

fn provider_error(err: ProviderError) -> ApiError {
    match err {
        ProviderError::UnknownIntent(id) => ApiError::unknown_intent(&id),
        ProviderError::AlreadyRefunded(id) => ApiError::conflict("already_refunded", id),
        ProviderError::ProviderUnavailable => {
            ApiError::service_unavailable("provider temporarily unavailable")
        }
        ProviderError::Validation(msg) => ApiError::conflict("provider_state", msg),
    }
}

fn provider_refund_error(err: ProviderError) -> ApiError {
    provider_error(err)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Intent registry stored on AppState (gateway-level link from intent to
/// the account it funds).
pub(crate) type IntentAccounts = Arc<RwLock<HashMap<String, String>>>;
