//! x402 v2 transport over the deposit-intent ledger, plus facilitator
//! endpoints for the NEAR client-submitted payment-proof method.
//!
//! Pinned specification: x402 v2 (`specs/x402-specification-v2.md` at
//! x402-foundation/x402 commit e187dda1ef, `specs/transports-v2/http.md`
//! at the same commit, the `exact` scheme families in
//! `specs/schemes/exact/scheme_exact.md` at 8228bd6d83, and the NEAR
//! binding `scheme_exact_near.md` at f5dd40efb4). See
//! docs/protocols/x402-near.md for the profile this module implements.
//!
//! Headers (base64-encoded JSON, exactly as the HTTP transport names
//! them): `PAYMENT-REQUIRED` (server to client, with 402),
//! `PAYMENT-SIGNATURE` (client to server, a full v2 `PaymentPayload`),
//! `PAYMENT-RESPONSE` (server to client, a `SettlementResponse`).
//!
//! Method: scheme `exact`, `extra.assetTransferMethod = "near-tx-proof"`,
//! `extra.paymentFlow = "upfront"` (the client-submitted family MUST use
//! upfront). The flow:
//!
//! 1. `POST /v2/scan` without a payment header creates a fresh deposit
//!    intent and answers 402 with requirements whose `extra.memo` is the
//!    intent id: a server-issued, single-use nonce the payment carries.
//! 2. The client pays: a direct `ft_transfer`, or a policy-wallet `pay`,
//!    to `payTo` with that memo.
//! 3. The client retries with `PAYMENT-SIGNATURE`, whose `payload` is
//!    `{network, intent_id, tx_hash, signer_id}`.
//! 4. The server verifies the receipt tree (aps_near::verify), funds the
//!    intent through the same idempotent path as the payment-intent
//!    routes, reserves, scans, settles, and answers 200 with
//!    `PAYMENT-RESPONSE`.
//!
//! There is no second funding path: the deposit uses the idempotency key
//! `intent:<id>` shared with `POST /payment/intents/{id}/observations`,
//! and reconciliation is the pure `aps_payment::reconcile`.

// Handlers in this module return a finished HTTP response on both paths
// (a payment failure carries headers, not just an error body), so the
// error type is axum's Response. It is built once per request; boxing it
// would buy nothing.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aps_core::{Amount, ReportHash, SCAN_SKU};
use aps_payment::{
    AssetId, CreateIntent, DepositIntent, IntentId, ProviderError, ReconcileOutcome,
};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use serde_json::{json, Value};

use crate::dto::{AmountJson, SettlementJson};
use crate::error::ApiError;
use crate::payment::{observe_and_fund, FundingResult};
use crate::scanbridge::ScanOutcome;
use crate::AppState;

/// The protocol version this module speaks.
pub const X402_VERSION: u64 = 2;
/// Server to client, with 402: base64 JSON `PaymentRequired`.
pub const HEADER_PAYMENT_REQUIRED: &str = "payment-required";
/// Client to server: base64 JSON `PaymentPayload`.
pub const HEADER_PAYMENT_SIGNATURE: &str = "payment-signature";
/// Server to client: base64 JSON `SettlementResponse`.
pub const HEADER_PAYMENT_RESPONSE: &str = "payment-response";
/// The scheme: `exact`.
pub const SCHEME: &str = "exact";
/// The proposed NEAR client-submitted method (not upstream).
pub const TRANSFER_METHOD: &str = "near-tx-proof";
/// Client-submitted methods MUST use the upfront flow.
pub const PAYMENT_FLOW: &str = "upfront";
/// The paid resource.
pub const RESOURCE_PATH: &str = "/v2/scan";
/// Default validity window of a requirement (and its deposit intent).
pub const DEFAULT_MAX_TIMEOUT_SECS: u64 = 900;
/// Seconds a client should wait before retrying a pending settlement.
pub const RETRY_AFTER_SECS: u64 = 3;

/// Static x402 configuration of this server.
#[derive(Debug, Clone)]
pub struct X402Config {
    /// CAIP-2-style network id: `near:mainnet`, `near:testnet` or the
    /// development-only `aps:mock`.
    pub network: String,
    /// The settlement asset (NEP-141 contract id for NEAR).
    pub asset: String,
    /// Price of one `scan.v1` call, in atomic units of the asset.
    pub price: Amount,
    /// Validity window of a requirement, in seconds.
    pub max_timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Proof {
    tx_hash: String,
    signer_id: String,
}

#[derive(Debug, Clone)]
struct Settled {
    request: Value,
    body: Value,
    payment_response: Value,
}

#[derive(Debug, Clone)]
struct IntentRecord {
    caller: String,
    account: String,
    amount: Amount,
    pay_to: String,
    /// The proof that funded this intent, once claimed.
    proof: Option<Proof>,
    /// Scan attempts that ended without a settlement (refunded jobs).
    attempts: u32,
    settled: Option<Settled>,
}

#[derive(Debug, Default)]
struct Book {
    intents: HashMap<String, IntentRecord>,
    /// Consumption keys (`<network>:<tx_hash>`) to their owner tag.
    consumed: HashMap<String, String>,
}

/// Process-local x402 state: issued requirements, claimed proofs, and
/// per-intent serialization locks.
#[derive(Debug)]
pub struct X402State {
    config: X402Config,
    book: Mutex<Book>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl X402State {
    /// Build the state for a configuration.
    pub fn new(config: X402Config) -> Self {
        Self {
            config,
            book: Mutex::new(Book::default()),
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// The configuration in use.
    pub fn config(&self) -> &X402Config {
        &self.config
    }

    fn book(&self) -> Result<std::sync::MutexGuard<'_, Book>, Response> {
        self.book
            .lock()
            .map_err(|_| ApiError::internal("x402 state is unavailable").into_response())
    }

    fn lock_for(&self, intent_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        match self.locks.lock() {
            Ok(mut locks) => Arc::clone(locks.entry(intent_id.to_string()).or_default()),
            Err(_) => Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Base64 (standard alphabet, padded) of a JSON value.
pub fn encode_header(value: &Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(value.to_string().as_bytes())
}

/// Decode a base64 JSON header; accepts the standard and URL-safe
/// alphabets, padded or not.
pub fn decode_header(raw: &str) -> Result<Value, String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let raw = raw.trim();
    let bytes = STANDARD
        .decode(raw)
        .or_else(|_| STANDARD_NO_PAD.decode(raw))
        .or_else(|_| URL_SAFE.decode(raw))
        .or_else(|_| URL_SAFE_NO_PAD.decode(raw))
        .map_err(|_| "the header is not valid base64".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "the header does not decode to JSON".to_string())
}

fn header_value(value: &Value) -> HeaderValue {
    // Base64 output is always a valid header value.
    HeaderValue::from_str(&encode_header(value)).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// A `SettlementResponse` (core spec 5.3).
fn settlement_response(
    success: bool,
    error_reason: Option<&str>,
    transaction: &str,
    network: &str,
    payer: Option<&str>,
) -> Value {
    let mut value = json!({
        "success": success,
        "transaction": transaction,
        "network": network,
    });
    if let Some(reason) = error_reason {
        value["errorReason"] = json!(reason);
    }
    if let Some(payer) = payer {
        value["payer"] = json!(payer);
    }
    value
}

/// A failed payment attempt: the standard error body plus a
/// `PAYMENT-RESPONSE` header carrying the same reason.
fn payment_failure(
    status: StatusCode,
    reason: &'static str,
    message: impl Into<String>,
    network: &str,
    transaction: &str,
) -> Response {
    let message = message.into();
    let header = settlement_response(false, Some(reason), transaction, network, None);
    let mut response = (
        status,
        Json(json!({ "error": { "code": reason, "message": message } })),
    )
        .into_response();
    response.headers_mut().insert(
        HeaderName::from_static(HEADER_PAYMENT_RESPONSE),
        header_value(&header),
    );
    if reason == "settlement_pending" {
        response.headers_mut().insert(
            axum::http::header::RETRY_AFTER,
            HeaderValue::from(RETRY_AFTER_SECS),
        );
    }
    response
}

/// One `accepts[]` entry for an issued intent.
fn requirement(config: &X402Config, intent: &DepositIntent) -> Value {
    json!({
        "scheme": SCHEME,
        "network": config.network,
        "amount": intent.amount.atomic().to_string(),
        "asset": config.asset,
        "payTo": intent.deposit_address,
        "maxTimeoutSeconds": config.max_timeout_secs,
        "extra": {
            "assetTransferMethod": TRANSFER_METHOD,
            "paymentFlow": PAYMENT_FLOW,
            "memo": intent.intent_id.as_str(),
            "intentId": intent.intent_id.as_str(),
            "expiresAt": intent.expires_at_unix,
            "sku": SCAN_SKU,
        }
    })
}

fn payment_required(config: &X402Config, intent: &DepositIntent, error: &str) -> Value {
    json!({
        "x402Version": X402_VERSION,
        "error": error,
        "resource": {
            "url": RESOURCE_PATH,
            "description": "TriagePay finding triage scan (scan.v1)",
            "mimeType": "application/json",
            "serviceName": "TriagePay",
        },
        "accepts": [requirement(config, intent)],
    })
}

/// The proof carried in `PaymentPayload.payload`.
#[derive(Debug, Clone)]
struct ProofPayload {
    accepted: Value,
    network: String,
    intent_id: String,
    tx_hash: String,
    signer_id: String,
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Parse a decoded `PaymentPayload`. Shape errors are 400 (malformed),
/// per the HTTP transport.
fn parse_payment_payload(payload: &Value, network: &str) -> Result<ProofPayload, Response> {
    let bad = |reason: &'static str, message: &str| {
        payment_failure(StatusCode::BAD_REQUEST, reason, message, network, "")
    };
    if !payload.is_object() {
        return Err(bad(
            "invalid_payload",
            "PaymentPayload must be a JSON object",
        ));
    }
    if payload.get("x402Version").and_then(Value::as_u64) != Some(X402_VERSION) {
        return Err(bad("invalid_x402_version", "x402Version must be 2"));
    }
    let Some(accepted) = payload.get("accepted").filter(|a| a.is_object()) else {
        return Err(bad(
            "invalid_payload",
            "PaymentPayload.accepted is required",
        ));
    };
    let Some(inner) = payload.get("payload").filter(|p| p.is_object()) else {
        return Err(bad("invalid_payload", "PaymentPayload.payload is required"));
    };
    let field = |key: &str| {
        str_at(inner, key)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let (Some(proof_network), Some(intent_id), Some(tx_hash), Some(signer_id)) = (
        field("network"),
        field("intent_id"),
        field("tx_hash"),
        field("signer_id"),
    ) else {
        return Err(bad(
            "invalid_payload",
            "payload must carry non-empty network, intent_id, tx_hash and signer_id",
        ));
    };
    Ok(ProofPayload {
        accepted: accepted.clone(),
        network: proof_network,
        intent_id,
        tx_hash,
        signer_id,
    })
}

/// Check that the accepted requirements are the ones this server
/// issued for the intent. Returns the failing reason and message.
fn check_accepted(
    config: &X402Config,
    accepted: &Value,
    memo: &str,
    pay_to: &str,
    amount: Amount,
) -> Result<(), (&'static str, String)> {
    if str_at(accepted, "scheme") != Some(SCHEME) {
        return Err((
            "invalid_scheme",
            "only the exact scheme is accepted".to_string(),
        ));
    }
    if str_at(accepted, "network") != Some(config.network.as_str()) {
        return Err((
            "invalid_network",
            format!("this server settles on {}", config.network),
        ));
    }
    let extra = accepted.get("extra").cloned().unwrap_or(Value::Null);
    if str_at(&extra, "assetTransferMethod") != Some(TRANSFER_METHOD) {
        return Err((
            "invalid_payment_requirements",
            format!("extra.assetTransferMethod must be {TRANSFER_METHOD}"),
        ));
    }
    if let Some(flow) = str_at(&extra, "paymentFlow") {
        if flow != PAYMENT_FLOW {
            return Err((
                "invalid_payment_requirements",
                "extra.paymentFlow must be upfront".to_string(),
            ));
        }
    }
    if str_at(&extra, "memo") != Some(memo) {
        return Err((
            "invalid_payment_requirements",
            "extra.memo must equal the payload intent_id".to_string(),
        ));
    }
    if str_at(accepted, "asset") != Some(config.asset.as_str()) {
        return Err((
            "invalid_payment_requirements",
            "the accepted asset is not this server's settlement asset".to_string(),
        ));
    }
    if str_at(accepted, "payTo") != Some(pay_to) {
        return Err((
            "invalid_payment_requirements",
            "the accepted payTo is not the issued payee".to_string(),
        ));
    }
    if str_at(accepted, "amount") != Some(amount.atomic().to_string().as_str()) {
        return Err((
            "invalid_payment_requirements",
            "the accepted amount is not the issued amount".to_string(),
        ));
    }
    Ok(())
}

/// `POST /v2/scan`: the paid scan behind HTTP 402.
pub async fn scan_v2(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    match scan_v2_inner(&state, &headers, &body).await {
        Ok(response) | Err(response) => response,
    }
}

async fn scan_v2_inner(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Response, Response> {
    let x402 = state.x402();
    let network = x402.config().network.clone();
    let raw = std::str::from_utf8(body)
        .map_err(|_| ApiError::validation("request body must be UTF-8 JSON").into_response())?;
    let request: Value = serde_json::from_str(raw)
        .map_err(|_| ApiError::validation("request body must be valid JSON").into_response())?;
    if !request.is_object() {
        return Err(ApiError::validation("request body must be a JSON object").into_response());
    }
    let Some(signature) = headers.get(HEADER_PAYMENT_SIGNATURE) else {
        return issue_requirement(state).await;
    };
    let decoded = signature
        .to_str()
        .map_err(|_| "the header is not ASCII".to_string())
        .and_then(decode_header)
        .map_err(|message| {
            payment_failure(
                StatusCode::BAD_REQUEST,
                "invalid_payload",
                message,
                &network,
                "",
            )
        })?;
    let proof = parse_payment_payload(&decoded, &network)?;
    let fail = |reason: &'static str, message: String| {
        payment_failure(
            StatusCode::PAYMENT_REQUIRED,
            reason,
            message,
            &network,
            &proof.tx_hash,
        )
    };
    if proof.network != network {
        return Err(fail(
            "invalid_network",
            format!("this server settles on {network}"),
        ));
    }

    // Serialize every presentation for one intent: of two concurrent
    // presentations, exactly one claims the proof.
    let lock = x402.lock_for(&proof.intent_id);
    let _guard = lock.lock().await;

    let record = x402.book()?.intents.get(&proof.intent_id).cloned();
    let Some(mut record) = record else {
        return Err(fail(
            "invalid_payload",
            "unknown intent: request a fresh requirement".to_string(),
        ));
    };
    if record.caller != state.caller() {
        return Err(fail(
            "invalid_payload",
            "the requirement was issued to a different caller".to_string(),
        ));
    }
    if let Err((reason, message)) = check_accepted(
        x402.config(),
        &proof.accepted,
        &proof.intent_id,
        &record.pay_to,
        record.amount,
    ) {
        return Err(fail(reason, message));
    }
    let binding = Proof {
        tx_hash: proof.tx_hash.clone(),
        signer_id: proof.signer_id.clone(),
    };
    let intent_id = IntentId::new(proof.intent_id.clone());

    // A proof already claimed for this intent: replay or refuse.
    if let Some(claimed) = &record.proof {
        if claimed.tx_hash != binding.tx_hash {
            record_foreign_proof(state, &intent_id, &binding).await;
            return Err(fail(
                "proof_already_consumed",
                "this intent is already funded by another transaction; a matching transfer \
                 in this one is recorded as an orphan deposit for operator refund"
                    .to_string(),
            ));
        }
        if let Some(settled) = &record.settled {
            if settled.request == request {
                return Ok(paid_response(&settled.body, &settled.payment_response));
            }
            return Err(fail(
                "proof_already_consumed",
                "this payment proof was consumed by a different request".to_string(),
            ));
        }
    }
    let consumption_key = format!("{network}:{}", binding.tx_hash);
    {
        let book = x402.book()?;
        if let Some(owner) = book.consumed.get(&consumption_key) {
            if owner != &proof.intent_id {
                return Err(fail(
                    "proof_already_consumed",
                    "this transaction was already consumed by another payment".to_string(),
                ));
            }
        }
    }

    // Verify and fund through the one funding path.
    let hint = (state.provider_kind() == "near").then(|| aps_near::TxHint {
        signer_id: binding.signer_id.clone(),
        tx_hash: binding.tx_hash.clone(),
    });
    let funding = if record.proof.is_some() {
        // Already funded by this proof (an earlier attempt's scan failed):
        // observe again without a new hint; the observation is final.
        observe_and_fund(state, &intent_id, &record.account, None).await
    } else {
        observe_and_fund(state, &intent_id, &record.account, hint).await
    };
    let FundingResult {
        outcome, intent, ..
    } = match funding {
        Ok(result) => result,
        Err(ProviderError::ProviderUnavailable) => {
            return Err(fail(
                "settlement_pending",
                "the chain could not be read; retry the same proof".to_string(),
            ))
        }
        Err(err) => return Err(fail("invalid_payload", err.to_string())),
    };
    match &outcome {
        ReconcileOutcome::Fund { .. } => {}
        ReconcileOutcome::StillPending => {
            return Err(fail(
                "settlement_pending",
                "the transaction is not final yet; retry the same proof".to_string(),
            ))
        }
        ReconcileOutcome::Mismatch {
            observed_amount, ..
        } => {
            return Err(fail(
                "amount_mismatch",
                format!(
                    "the transfer moved {} atomic units where {} were required; it is recorded \
                     as an orphan deposit for operator refund",
                    observed_amount.atomic(),
                    intent.amount.atomic()
                ),
            ))
        }
        ReconcileOutcome::Expired => {
            return Err(fail(
                "requirement_expired",
                "the requirement expired before a final payment was observed".to_string(),
            ))
        }
        ReconcileOutcome::Rejected { reason } => {
            return Err(fail(
                "invalid_transaction_state",
                format!("the payment transaction failed: {reason}"),
            ))
        }
    }

    // Claim the proof atomically (under the intent lock and the book lock).
    {
        let mut book = x402.book()?;
        if let Some(owner) = book.consumed.get(&consumption_key) {
            if owner != &proof.intent_id {
                return Err(fail(
                    "proof_already_consumed",
                    "this transaction was already consumed by another payment".to_string(),
                ));
            }
        }
        book.consumed
            .insert(consumption_key.clone(), proof.intent_id.clone());
        record.proof = Some(binding.clone());
        book.intents.insert(proof.intent_id.clone(), record.clone());
    }
    let payer = state
        .with_payment(|provider| provider.evidence(&intent_id))
        .await
        .map(|evidence| evidence.payer_id)
        .unwrap_or_else(|| binding.signer_id.clone());

    run_paid_scan(
        state,
        raw,
        &request,
        &proof.intent_id,
        record,
        &binding,
        &payer,
    )
    .await
}

/// Reserve, scan and settle for a funded intent; record the response for
/// idempotent replay.
async fn run_paid_scan(
    state: &AppState,
    raw: &str,
    request: &Value,
    intent_id: &str,
    mut record: IntentRecord,
    binding: &Proof,
    payer: &str,
) -> Result<Response, Response> {
    let x402 = state.x402();
    let network = x402.config().network.clone();
    let idem = format!("x402:{intent_id}:{}", record.attempts);
    let job = {
        let mut ledger = state.write_ledger().await;
        let reservation = ledger
            .reserve(
                state.caller(),
                &record.account,
                SCAN_SKU,
                record.amount,
                &idem,
            )
            .map_err(|err| ApiError::from(err).into_response())?;
        state
            .register_job(&reservation.job_id)
            .map_err(IntoResponse::into_response)?;
        ledger
            .start(&reservation.job_id)
            .map_err(|err| ApiError::from(err).into_response())?;
        reservation.job_id
    };

    let outcome = state.scan_launcher().launch(raw).await;
    let (report, content_hash) = match outcome {
        Ok(ScanOutcome::Report {
            report,
            content_hash,
        }) => (report, content_hash),
        Ok(ScanOutcome::InvalidInput { errors }) => {
            release_job(
                state,
                &job,
                &mut record,
                intent_id,
                "envelope failed validation",
            )
            .await?;
            let joined: String = errors
                .iter()
                .map(|e| e.chars().take(120).collect::<String>())
                .collect::<Vec<_>>()
                .join("; ")
                .chars()
                .take(500)
                .collect();
            return Err(ApiError::validation(format!(
                "envelope failed validation: {joined}; the payment is retained - retry with the \
                 same PAYMENT-SIGNATURE and a corrected body"
            ))
            .into_response());
        }
        Err(err) => {
            release_job(state, &job, &mut record, intent_id, "scanner failure").await?;
            return Err(ApiError::internal(format!(
                "the scanner failed ({err:?}); the payment is retained - retry with the same \
                 PAYMENT-SIGNATURE"
            ))
            .into_response());
        }
    };
    let hash = ReportHash::from_hex(&content_hash).map_err(|_| {
        ApiError::internal("scanner produced an invalid report hash").into_response()
    })?;
    let settlement = {
        let mut ledger = state.write_ledger().await;
        ledger
            .settle(&job, hash)
            .map_err(|err| ApiError::from(err).into_response())?
    };
    let amount = record.amount;
    let mut payment_response =
        settlement_response(true, None, &binding.tx_hash, &network, Some(payer));
    payment_response["amount"] = json!(amount.atomic().to_string());
    payment_response["extensions"] = json!({
        "aps-ledger": {
            "intentId": intent_id,
            "jobId": job.to_string(),
            "reportHash": content_hash,
        }
    });
    let body = json!({
        "report": report,
        "settlement": SettlementJson::from(settlement),
        "payment": {
            "intent_id": intent_id,
            "network": network,
            "transaction": binding.tx_hash,
            "signer_id": binding.signer_id,
            "payer": payer,
            "amount": AmountJson::from(amount),
            "job_id": job.to_string(),
        },
    });
    record.settled = Some(Settled {
        request: request.clone(),
        body: body.clone(),
        payment_response: payment_response.clone(),
    });
    x402.book()?.intents.insert(intent_id.to_string(), record);
    Ok(paid_response(&body, &payment_response))
}

/// Refund a job whose scan did not complete and count the attempt; the
/// funded credit stays in the x402 account for a retry with the same
/// proof.
async fn release_job(
    state: &AppState,
    job: &aps_core::JobId,
    record: &mut IntentRecord,
    intent_id: &str,
    reason: &str,
) -> Result<(), Response> {
    {
        let mut ledger = state.write_ledger().await;
        ledger
            .refund(job, reason)
            .map_err(|err| ApiError::from(err).into_response())?;
    }
    record.attempts += 1;
    state
        .x402()
        .book()?
        .intents
        .insert(intent_id.to_string(), record.clone());
    Ok(())
}

fn paid_response(body: &Value, payment_response: &Value) -> Response {
    let mut response = (StatusCode::OK, Json(body.clone())).into_response();
    response.headers_mut().insert(
        HeaderName::from_static(HEADER_PAYMENT_RESPONSE),
        header_value(payment_response),
    );
    response
}

async fn record_foreign_proof(state: &AppState, intent_id: &IntentId, proof: &Proof) {
    if state.provider_kind() != "near" {
        return;
    }
    let hint = aps_near::TxHint {
        signer_id: proof.signer_id.clone(),
        tx_hash: proof.tx_hash.clone(),
    };
    let mut provider = state.payment().lock().await;
    // Best effort: a failed probe changes nothing about the funding.
    let _ = provider.record_foreign_proof(intent_id, hint).await;
}

/// No payment header: create a fresh deposit intent and answer 402.
async fn issue_requirement(state: &AppState) -> Result<Response, Response> {
    let x402 = state.x402();
    let config = x402.config().clone();
    let now = unix_now();
    let create = CreateIntent {
        asset_id: AssetId::new(config.asset.clone()),
        amount: config.price,
        expires_at_unix: now.saturating_add(config.max_timeout_secs),
    };
    let intent = {
        let mut provider = state.payment().lock().await;
        provider.sync_clock(now);
        provider.create_intent(create, None).await.map_err(|err| {
            ApiError::internal(format!("cannot issue a payment requirement: {err}")).into_response()
        })?
    };
    let account = format!("x402:{}", state.caller());
    state
        .register_intent(&intent.intent_id, &account)
        .map_err(IntoResponse::into_response)?;
    x402.book()?.intents.insert(
        intent.intent_id.to_string(),
        IntentRecord {
            caller: state.caller().to_string(),
            account,
            amount: intent.amount,
            pay_to: intent.deposit_address.clone(),
            proof: None,
            attempts: 0,
            settled: None,
        },
    );
    let required = payment_required(&config, &intent, "PAYMENT-SIGNATURE header is required");
    let mut response = (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": {
                "code": "payment_required",
                "message": "pay the requirement in the PAYMENT-REQUIRED header, then retry with PAYMENT-SIGNATURE",
            },
            "payment_required": required,
        })),
    )
        .into_response();
    response.headers_mut().insert(
        HeaderName::from_static(HEADER_PAYMENT_REQUIRED),
        header_value(&required),
    );
    Ok(response)
}

// ----- facilitator endpoints ------------------------------------------------

/// `GET /x402/supported` (core spec 7.3).
pub async fn supported(State(state): State<AppState>) -> Json<Value> {
    let config = state.x402().config().clone();
    Json(json!({
        "kinds": [{
            "x402Version": X402_VERSION,
            "scheme": SCHEME,
            "network": config.network,
            "extra": {
                "assetTransferMethod": TRANSFER_METHOD,
                "paymentFlow": PAYMENT_FLOW,
                "asset": config.asset,
            }
        }],
        "extensions": [],
        "signers": {},
    }))
}

struct FacilitatorOk {
    tx_hash: String,
    payer: String,
    amount: String,
    memo: String,
}

/// Validate a facilitator request and verify its proof on chain.
/// Read-only. The error carries (reason, transaction).
async fn facilitator_check(
    state: &AppState,
    request: &Value,
) -> Result<FacilitatorOk, (&'static str, String)> {
    let x402 = state.x402();
    let config = x402.config();
    if request.get("x402Version").and_then(Value::as_u64) != Some(X402_VERSION) {
        return Err(("invalid_x402_version", String::new()));
    }
    let requirements = request
        .get("paymentRequirements")
        .filter(|r| r.is_object())
        .ok_or(("invalid_payment_requirements", String::new()))?;
    let payload = request
        .get("paymentPayload")
        .ok_or(("invalid_payload", String::new()))?;
    let proof = parse_payment_payload(payload, &config.network)
        .map_err(|_| ("invalid_payload", String::new()))?;
    let tx = proof.tx_hash.clone();
    if proof.network != config.network {
        return Err(("invalid_network", tx));
    }
    let memo = str_at(requirements.get("extra").unwrap_or(&Value::Null), "memo")
        .filter(|m| !m.is_empty())
        .ok_or(("invalid_payment_requirements", tx.clone()))?
        .to_string();
    let pay_to = str_at(requirements, "payTo")
        .filter(|p| !p.is_empty())
        .ok_or(("invalid_payment_requirements", tx.clone()))?
        .to_string();
    let amount: u128 = str_at(requirements, "amount")
        .and_then(|a| a.parse().ok())
        .ok_or(("invalid_payment_requirements", tx.clone()))?;
    let asset = str_at(requirements, "asset")
        .filter(|a| !a.is_empty())
        .ok_or(("invalid_payment_requirements", tx.clone()))?
        .to_string();
    // The presented `accepted` must be exactly the requirements under
    // verification (the facilitator verifies one bound pair).
    let facilitator_config = X402Config {
        network: config.network.clone(),
        asset: asset.clone(),
        price: Amount::from_atomic(amount),
        max_timeout_secs: config.max_timeout_secs,
    };
    if let Err((reason, _)) = check_accepted(
        &facilitator_config,
        requirements,
        &memo,
        &pay_to,
        Amount::from_atomic(amount),
    ) {
        return Err((reason, tx));
    }
    if proof.accepted != *requirements {
        return Err(("invalid_payment_requirements", tx));
    }
    if proof.intent_id != memo {
        return Err(("invalid_payload", tx));
    }
    // Intents issued by this server's own /v2/scan are settled there
    // only: a third party cannot consume their proofs here first.
    if x402
        .book()
        .map(|book| book.intents.contains_key(&memo))
        .unwrap_or(true)
    {
        return Err(("invalid_payment_requirements", tx));
    }
    let rpc = state
        .with_payment(|provider| provider.near_rpc())
        .await
        .ok_or(("invalid_network", tx.clone()))?;
    let intent = DepositIntent {
        intent_id: IntentId::new(memo.clone()),
        deposit_address: pay_to,
        asset_id: AssetId::new(asset),
        amount: Amount::from_atomic(amount),
        expires_at_unix: u64::MAX,
        status: aps_payment::IntentStatus::Pending,
    };
    let hint = aps_near::TxHint {
        signer_id: proof.signer_id.clone(),
        tx_hash: proof.tx_hash.clone(),
    };
    let check =
        tokio::task::spawn_blocking(move || aps_near::verify_tx_receipts(rpc, &intent, &hint))
            .await
            .map_err(|_| ("unexpected_verify_error", tx.clone()))?
            .map_err(|err| match err {
                ProviderError::ProviderUnavailable => ("settlement_pending", tx.clone()),
                _ => ("unexpected_verify_error", tx.clone()),
            })?;
    match check.verification {
        aps_near::Verification::Confirmed { amount_atomic, .. } => {
            if amount_atomic != amount.to_string() {
                return Err(("amount_mismatch", tx));
            }
            let payer = check
                .evidence
                .map(|e| e.payer_id)
                .unwrap_or_else(|| proof.signer_id.clone());
            Ok(FacilitatorOk {
                tx_hash: tx,
                payer,
                amount: amount_atomic,
                memo,
            })
        }
        aps_near::Verification::NotFinalYet => Err(("settlement_pending", tx)),
        aps_near::Verification::Failed(_) => Err(("invalid_transaction_state", tx)),
        aps_near::Verification::StructuralMismatch(_) => Err(("invalid_payload", tx)),
    }
}

fn parse_facilitator_body(body: &[u8]) -> Result<Value, Response> {
    let request: Value = serde_json::from_slice(body).map_err(|_| {
        ApiError::validation("request body must be a JSON facilitator request").into_response()
    })?;
    if !request.is_object() {
        return Err(ApiError::validation("request body must be a JSON object").into_response());
    }
    Ok(request)
}

/// `POST /x402/verify` (core spec 7.1): read-only verification of a
/// near-tx-proof payment against third-party requirements.
pub async fn verify(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match parse_facilitator_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let consumed = |tx: &str| {
        state
            .x402()
            .book()
            .map(|book| {
                book.consumed
                    .contains_key(&format!("{}:{tx}", state.x402().config().network))
            })
            .unwrap_or(false)
    };
    let value = match facilitator_check(&state, &request).await {
        Ok(ok) if consumed(&ok.tx_hash) => json!({
            "isValid": false,
            "invalidReason": "proof_already_consumed",
            "payer": ok.payer,
        }),
        Ok(ok) => json!({ "isValid": true, "payer": ok.payer }),
        Err((reason, _)) => json!({ "isValid": false, "invalidReason": reason }),
    };
    Json(value).into_response()
}

/// `POST /x402/settle` (core spec 7.2): for a client-submitted proof,
/// settle validates the proof and binds it by consuming its key
/// (`<network>:<tx_hash>`). Of two settles of one proof, exactly one
/// succeeds.
pub async fn settle(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match parse_facilitator_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let network = state.x402().config().network.clone();
    let value = match facilitator_check(&state, &request).await {
        Ok(ok) => {
            let key = format!("{network}:{}", ok.tx_hash);
            let claimed = match state.x402().book() {
                Ok(mut book) => match book.consumed.entry(key) {
                    std::collections::hash_map::Entry::Occupied(_) => false,
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(format!("facilitator:{}", ok.memo));
                        true
                    }
                },
                Err(response) => return response,
            };
            if claimed {
                let mut value =
                    settlement_response(true, None, &ok.tx_hash, &network, Some(&ok.payer));
                value["amount"] = json!(ok.amount);
                value
            } else {
                settlement_response(
                    false,
                    Some("proof_already_consumed"),
                    &ok.tx_hash,
                    &network,
                    Some(&ok.payer),
                )
            }
        }
        Err((reason, tx)) => settlement_response(false, Some(reason), &tx, &network, None),
    };
    Json(value).into_response()
}
