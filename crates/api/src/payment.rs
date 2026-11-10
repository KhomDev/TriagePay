//! Payment-intent routes: funding credit from provider observations.
//!
//! This is the anti-spoofing surface of the gateway. A caller can create
//! a deposit intent and poll it, but credit enters an account ONLY when
//! the provider itself observes a confirmation that reconciles exactly
//! (same amount, same asset) against the intent. A client assertion of
//! "I paid" has no route into the ledger.
//!
//! Two providers sit behind one enum:
//!
//! - the deterministic mock (default; the scenario field is a test and
//!   demo control surface that would not exist on a real adapter), and
//! - the NEAR adapter, where the caller pays a NEP-141 ft_transfer to
//!   the gateway account with the intent id as the memo and submits the
//!   transaction hash as a hint. The adapter re-verifies every fact on
//!   chain (status, contract, method, arguments, memo, deposit, amount)
//!   before producing an observation; the chain fetch runs on the
//!   blocking thread pool so the async runtime is never blocked.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use aps_payment::{
    reconcile, AssetId, CreateIntent, DepositIntent, IntentId, MockProvider, MockScenario,
    Observation, PaymentProvider, ProviderError, ReconcileOutcome, RefundRequest,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::dto::{AmountJson, DepositReceiptJson};
use crate::error::ApiError;
use crate::AppState;

/// The caller identity the gateway itself acts under when funding
/// credit from a provider confirmation.
const GATEWAY_CALLER: &str = "payment-gateway";

/// The asset id the mock provider settles x402 requirements in.
pub const MOCK_X402_ASSET: &str = "mock:usdc";

/// The active payment provider. The mock is the default; the NEAR
/// adapter activates only through explicit environment configuration
/// (APS_PROVIDER=near and friends), never implicitly.
pub enum Provider {
    /// Deterministic in-memory provider: the default for tests and demos.
    Mock(MockProvider),
    /// NEAR adapter: on-chain deposit verification over JSON-RPC.
    Near(aps_near::NearProvider),
}

impl Provider {
    /// The provider kind, surfaced in API responses.
    pub fn kind(&self) -> &'static str {
        match self {
            Provider::Mock(_) => "mock",
            Provider::Near(_) => "near",
        }
    }

    /// Create a deposit intent. Mock scenarios are a mock-only control
    /// surface and are rejected for the NEAR provider.
    pub async fn create_intent(
        &mut self,
        req: CreateIntent,
        mock_scenario: Option<MockScenario>,
    ) -> Result<DepositIntent, ProviderError> {
        match self {
            Provider::Mock(mock) => mock.create_intent_with_scenario(
                req,
                mock_scenario.unwrap_or(MockScenario::ConfirmOnPoll { polls: 1 }),
            ),
            Provider::Near(near) => {
                if mock_scenario.is_some() {
                    return Err(ProviderError::Validation(
                        "mock_scenario is only valid for the mock provider".to_string(),
                    ));
                }
                near.create_intent(req)
            }
        }
    }

    /// Observe an intent. For the NEAR provider the chain fetch runs on
    /// the blocking thread pool; observations are idempotent.
    pub async fn observe(&mut self, intent_id: &IntentId) -> Result<Observation, ProviderError> {
        match self {
            Provider::Mock(mock) => mock.observe(intent_id),
            Provider::Near(near) => match near.prepare_fetch(intent_id)? {
                aps_near::FetchStep::Ready(observation) => Ok(observation),
                aps_near::FetchStep::Verify { rpc, intent, hint } => {
                    let check = tokio::task::spawn_blocking(move || {
                        aps_near::verify_tx_receipts(rpc, &intent, &hint)
                    })
                    .await
                    .map_err(|err| {
                        ProviderError::Validation(format!("verification task failed: {err}"))
                    })??;
                    near.apply_check(intent_id, check)
                }
            },
        }
    }

    /// Request a refund. The mock executes it instantly; the NEAR
    /// provider returns a refund PROPOSAL that requires a human-approved
    /// on-chain transfer (no automatic payout, by design).
    pub async fn request_refund(
        &mut self,
        intent_id: &IntentId,
        reason: &str,
    ) -> Result<RefundRequest, ProviderError> {
        match self {
            Provider::Mock(mock) => mock.request_refund(intent_id, reason),
            Provider::Near(near) => near.request_refund(intent_id, reason),
        }
    }

    /// Submit a transaction hint for the NEAR provider. The mock
    /// provider has no use for hints and rejects them.
    pub async fn submit_hint(
        &mut self,
        intent_id: &IntentId,
        hint: aps_near::TxHint,
    ) -> Result<(), ProviderError> {
        match self {
            Provider::Mock(_) => Err(ProviderError::Validation(
                "the mock provider does not accept a transaction hint".to_string(),
            )),
            Provider::Near(near) => near.submit_hint(intent_id, hint),
        }
    }

    /// Read-only view of an intent, if present. Syncs the provider clock
    /// first so expiry status is current. Synchronous: safe to call inside
    /// closures handed to AppState::with_payment.
    pub fn intent(&mut self, intent_id: &IntentId) -> Result<Option<DepositIntent>, ProviderError> {
        let now = unix_now();
        match self {
            Provider::Mock(mock) => {
                mock.sync_clock(now);
                Ok(mock.intent(intent_id).cloned())
            }
            Provider::Near(near) => {
                near.sync_clock(now);
                Ok(near.intent(intent_id).cloned())
            }
        }
    }

    /// The x402 network identifier, settlement asset and payee this
    /// provider settles in. NEAR uses the CAIP-2-style ids `near:mainnet`
    /// and `near:testnet`; the mock provider uses `aps:mock`, a
    /// development-only network that no real x402 client would select.
    pub fn x402_identity(&self) -> (String, String, Option<String>) {
        match self {
            Provider::Mock(_) => ("aps:mock".to_string(), MOCK_X402_ASSET.to_string(), None),
            Provider::Near(near) => {
                let config = near.config();
                (
                    format!("near:{}", config.network),
                    config.asset_contract_id.clone(),
                    Some(config.gateway_account_id.clone()),
                )
            }
        }
    }

    /// The token-receipt evidence behind a NEAR confirmation.
    pub fn evidence(&self, intent_id: &IntentId) -> Option<aps_near::TransferEvidence> {
        match self {
            Provider::Mock(_) => None,
            Provider::Near(near) => near.evidence(intent_id).cloned(),
        }
    }

    /// Orphaned transfers recorded against a NEAR intent.
    pub fn orphans(&self, intent_id: &IntentId) -> Vec<aps_near::OrphanTransfer> {
        match self {
            Provider::Mock(_) => Vec::new(),
            Provider::Near(near) => near.orphans(intent_id).to_vec(),
        }
    }

    /// A read-only RPC handle (NEAR only), for the facilitator /verify.
    pub fn near_rpc(&self) -> Option<aps_near::rpc::NearRpc> {
        match self {
            Provider::Mock(_) => None,
            Provider::Near(near) => Some(near.rpc()),
        }
    }

    /// Check a transaction that is not the intent's own funding proof
    /// (a second payment for a settled intent) and record any matching
    /// transfer as an orphan. Never changes the funding observation.
    pub async fn record_foreign_proof(
        &mut self,
        intent_id: &IntentId,
        hint: aps_near::TxHint,
    ) -> Result<(), ProviderError> {
        let Provider::Near(near) = self else {
            return Ok(());
        };
        let aps_near::FetchStep::Verify { rpc, intent, hint } =
            near.prepare_probe(intent_id, hint)?
        else {
            return Ok(());
        };
        let check =
            tokio::task::spawn_blocking(move || aps_near::verify_tx_receipts(rpc, &intent, &hint))
                .await
                .map_err(|err| {
                    ProviderError::Validation(format!("verification task failed: {err}"))
                })??;
        near.apply_check(intent_id, check).map(|_| ())
    }

    /// Sync the provider clock to wall time.
    pub fn sync_clock(&mut self, now: u64) {
        match self {
            Provider::Mock(mock) => mock.sync_clock(now),
            Provider::Near(near) => near.sync_clock(now),
        }
    }
}

/// POST /payment/intents request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateIntentRequest {
    /// Account credited when a matching confirmation is observed.
    pub account_id: String,
    /// Asset identifier, opaque to the gateway. For the NEAR provider
    /// this must equal the configured settlement asset contract.
    pub asset_id: String,
    /// Amount in atomic units, as a decimal string.
    pub amount_atomic: String,
    /// How long the intent stays open, in seconds.
    pub expires_in_secs: u64,
    /// Mock-only behavior script. Absent means confirm on first poll.
    /// Rejected when the NEAR provider is active.
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
    /// Which provider answered: "mock" or "near".
    pub provider: &'static str,
    /// For the NEAR provider: the memo the payer must attach to the
    /// ft_transfer. Always equal to the intent id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

/// POST /payment/intents/{intent_id}/observations request body.
///
/// Optional: the mock provider observes without a body. The NEAR
/// provider requires the transaction hint - the hash of the payer's
/// ft_transfer and the signer id needed to look it up. Hints locate a
/// transaction; the provider re-verifies every fact on chain.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveHintRequest {
    pub signer_id: String,
    pub tx_hash: String,
}

/// Create a deposit intent.
///
/// 201 with the intent (including the deposit instructions the caller
/// pays to). Credit is NOT granted here; only a provider-observed,
/// exactly matching confirmation funds the account.
pub async fn create_intent(
    State(state): State<AppState>,
    body: Result<Json<CreateIntentRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<IntentJson>), ApiError> {
    let Json(request) = body.map_err(crate::handlers::rejected_json)?;
    let amount = crate::dto::parse_amount_atomic(&request.amount_atomic)?;
    let scenario = match request.mock_scenario {
        None => None,
        Some(scenario) => Some(MockScenario::try_from(scenario)?),
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
            .create_intent(req, scenario)
            .await
            .map_err(provider_create_error)?
    };
    state.register_intent(&intent.intent_id, &request.account_id)?;
    Ok((
        StatusCode::CREATED,
        Json(intent_json(
            &intent,
            &request.account_id,
            provider_kind(&state),
        )),
    ))
}

/// Fetch an intent.
pub async fn get_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
) -> Result<Json<IntentJson>, ApiError> {
    let account_id = state.intent_account(&intent_id)?;
    let intent = state
        .with_payment(|provider| {
            provider
                .intent(&IntentId::new(intent_id.clone()))
                .map_err(provider_error)
        })
        .await?
        .ok_or_else(|| ApiError::unknown_intent(&intent_id))?;
    Ok(Json(intent_json(
        &intent,
        &account_id,
        state.provider_kind(),
    )))
}

/// Observe an intent: poll the provider and reconcile.
///
/// 200 with the observation, the reconciliation outcome, and - only when
/// the provider confirmed a payment matching the intent exactly - the
/// deposit receipt that funded the account. Replays are safe: the
/// funding deposit is idempotent on the intent id, so observing twice
/// never credits twice.
pub async fn observe_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    // The raw body is parsed manually (like the scan route) so that a
    // bodyless poll - the mock provider's flow - never trips a
    // content-type rejection, while a present body is parsed strictly.
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let account_id = state.intent_account(&intent_id)?;
    let hint = normalize_hint_body(&body, state.provider_kind())?;
    let intent_id = IntentId::new(intent_id.clone());
    let funding = observe_and_fund(&state, &intent_id, &account_id, hint)
        .await
        .map_err(provider_error)?;
    let FundingResult {
        observation,
        outcome,
        deposit,
        ..
    } = funding;
    let mut body = json!({
        "intent_id": intent_id.as_str(),
        "provider": state.provider_kind(),
        "observation": observation_json(&observation),
        "reconciled": outcome_name(&outcome),
        "funded": false,
    });
    if let (ReconcileOutcome::Fund { tx_ref }, Some(receipt)) = (&outcome, deposit) {
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

/// What one observation of an intent produced.
pub(crate) struct FundingResult {
    pub observation: Observation,
    pub outcome: ReconcileOutcome,
    /// The ledger deposit, present exactly when `outcome` is Fund. It is
    /// idempotent on the intent id: replays return the recorded receipt.
    pub deposit: Option<aps_core::DepositReceipt>,
    pub intent: DepositIntent,
}

/// The one funding path: submit the hint (NEAR), observe the provider,
/// reconcile purely, and - only on an exact match - deposit the intent
/// amount into the account under the idempotency key `intent:<id>`.
/// Both the payment-intent routes and the x402 transport call this, so
/// an intent can never fund twice, whichever route observes it.
pub(crate) async fn observe_and_fund(
    state: &AppState,
    intent_id: &IntentId,
    account_id: &str,
    hint: Option<aps_near::TxHint>,
) -> Result<FundingResult, ProviderError> {
    let now = unix_now();
    let observation = {
        let mut provider = state.payment().lock().await;
        provider.sync_clock(now);
        if let Some(hint) = hint {
            provider.submit_hint(intent_id, hint).await?;
        }
        provider.observe(intent_id).await?
    };
    let intent = state
        .with_payment(|provider| provider.intent(intent_id))
        .await?
        .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))?;
    let outcome = reconcile(&intent, &observation);
    let deposit = if matches!(outcome, ReconcileOutcome::Fund { .. }) {
        let mut ledger = state.write_ledger().await;
        let receipt = ledger
            .deposit(
                GATEWAY_CALLER,
                account_id,
                intent.amount,
                &format!("intent:{intent_id}"),
            )
            .map_err(|err| ProviderError::Validation(format!("ledger deposit failed: {err}")))?;
        Some(receipt)
    } else {
        None
    };
    Ok(FundingResult {
        observation,
        outcome,
        deposit,
        intent,
    })
}

fn normalize_hint_body(
    body: &[u8],
    provider_kind: &str,
) -> Result<Option<aps_near::TxHint>, ApiError> {
    let raw = std::str::from_utf8(body)
        .map_err(|_| ApiError::validation("request body must be UTF-8 JSON"))?;
    if raw.trim().is_empty() {
        if provider_kind == "near" {
            return Err(ApiError::validation(
                "the near provider requires a transaction hint body with signer_id and tx_hash",
            ));
        }
        return Ok(None);
    }
    if provider_kind != "near" {
        return Err(ApiError::validation(
            "the mock provider does not accept a transaction hint body",
        ));
    }
    let request: ObserveHintRequest = serde_json::from_str(raw.trim()).map_err(|err| {
        ApiError::validation(format!(
            "request body must be a JSON object with signer_id and tx_hash: {err}"
        ))
    })?;
    if request.signer_id.trim().is_empty() || request.tx_hash.trim().is_empty() {
        return Err(ApiError::validation(
            "signer_id and tx_hash must be non-empty strings",
        ));
    }
    Ok(Some(aps_near::TxHint {
        signer_id: request.signer_id.trim().to_string(),
        tx_hash: request.tx_hash.trim().to_string(),
    }))
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
    body: Result<Json<IntentRefundRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(request) = body.map_err(crate::handlers::rejected_json)?;
    let _ = state.intent_account(&intent_id)?;
    let now = unix_now();
    let refund = {
        let mut provider = state.payment().lock().await;
        provider.sync_clock(now);
        provider
            .request_refund(&IntentId::new(intent_id.clone()), &request.reason)
            .await
            .map_err(provider_error)?
    };
    Ok(Json(json!({
        "refund_id": refund.refund_id,
        "intent_id": refund.intent_id.to_string(),
        "amount": AmountJson::from(refund.amount),
        "reason": refund.reason,
    })))
}

fn provider_kind(state: &AppState) -> &'static str {
    state.provider_kind()
}

fn intent_json(intent: &DepositIntent, account_id: &str, provider: &'static str) -> IntentJson {
    let memo = if provider == "near" {
        Some(intent.intent_id.to_string())
    } else {
        None
    };
    IntentJson {
        intent_id: intent.intent_id.to_string(),
        account_id: account_id.to_string(),
        deposit_address: intent.deposit_address.clone(),
        asset_id: intent.asset_id.to_string(),
        amount: AmountJson::from(intent.amount),
        expires_at_unix: intent.expires_at_unix,
        status: status_name(&intent.status),
        provider,
        memo,
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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Intent registry stored on AppState (gateway-level link from intent to
/// the account it funds).
pub(crate) type IntentAccounts = Arc<RwLock<HashMap<String, String>>>;
