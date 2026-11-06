//! Minimal blocking JSON-RPC client for the NEAR Protocol RPC interface.
//!
//! Only the read surface the adapter needs is implemented: node status,
//! view queries (account, access key), contract view calls
//! (`ft_balance_of`, `ft_metadata`), and finalized transaction status.
//! Every call is a plain HTTPS POST of a JSON-RPC 2.0 body; responses are
//! parsed defensively - a malformed answer is a typed error, never a
//! panic. The endpoint URL is stored but never rendered into an error
//! message, because privacy-preserving RPC endpoints embed API keys in
//! the path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

/// Failures of the RPC transport or protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcError {
    /// The endpoint URL is not usable (missing scheme, malformed).
    BadEndpoint(String),
    /// Network or HTTP-level failure, including timeouts.
    Transport(String),
    /// The endpoint answered, but the body is not a valid JSON-RPC response.
    Protocol(String),
    /// The node answered with a JSON-RPC error object. `cause` is the
    /// structured `error.cause.name` nearcore attaches (for example
    /// `METHOD_NOT_FOUND`, `TIMEOUT_ERROR`, `UNKNOWN_TRANSACTION`).
    Server {
        code: i64,
        message: String,
        data: Option<String>,
        cause: Option<String>,
    },
}

impl RpcError {
    /// True when the node reported a not-found condition for a query or
    /// transaction. The provider translates "not there (yet)" into a
    /// pending observation instead of surfacing an error.
    pub fn is_not_found(&self) -> bool {
        // "Method not found" is a protocol mismatch, not a missing object.
        if self.is_method_not_found() {
            return false;
        }
        match self {
            RpcError::Server {
                message,
                data,
                cause,
                ..
            } => {
                if cause.as_deref() == Some("UNKNOWN_TRANSACTION") {
                    return true;
                }
                let hay = format!("{message} {}", data.as_deref().unwrap_or(""));
                let lowered = hay.to_ascii_lowercase();
                lowered.contains("not found") || lowered.contains("does not exist")
            }
            _ => false,
        }
    }

    /// True when the node gave up waiting for the requested finality
    /// (`TIMEOUT_ERROR`). This is "not final yet", never a failure: the
    /// transaction may be unobserved, still executing, or (on a
    /// non-archival node) already garbage-collected.
    pub fn is_timeout(&self) -> bool {
        match self {
            RpcError::Server { cause, data, .. } => {
                cause.as_deref() == Some("TIMEOUT_ERROR")
                    || data
                        .as_deref()
                        .is_some_and(|d| d.eq_ignore_ascii_case("timeout"))
            }
            _ => false,
        }
    }

    /// True when the node does not know the requested JSON-RPC method
    /// (code -32601, cause `METHOD_NOT_FOUND`).
    pub fn is_method_not_found(&self) -> bool {
        match self {
            RpcError::Server { code, cause, .. } => {
                *code == -32601 || cause.as_deref() == Some("METHOD_NOT_FOUND")
            }
            _ => false,
        }
    }

    /// Human-readable description. Never includes the endpoint URL.
    pub fn describe(&self) -> String {
        match self {
            RpcError::BadEndpoint(message) => format!("bad RPC endpoint: {message}"),
            RpcError::Transport(message) => format!("RPC transport failure: {message}"),
            RpcError::Protocol(message) => format!("RPC protocol violation: {message}"),
            RpcError::Server { code, message, .. } => {
                format!("RPC server error {code}: {message}")
            }
        }
    }
}

/// Node status facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusInfo {
    /// Chain identifier, for example `mainnet` or `testnet`.
    pub chain_id: String,
    /// Latest block height known to the node.
    pub latest_block_height: u64,
}

/// `view_account` facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    /// Liquid NEAR balance in yoctoNEAR, as a decimal string.
    pub amount_yocto: String,
    /// Contract code hash; `11111111111111111111111111111111` for a plain
    /// account.
    pub code_hash: String,
    pub block_height: u64,
}

/// `view_access_key` facts. `permission` is kept as raw JSON: it is
/// either the string `"FullAccess"` or an object naming the allowed
/// contract, method, and allowance of a function-call key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessKeyView {
    pub nonce: u64,
    pub permission: Value,
}

/// Top-level (final) execution status of a transaction: nearcore's
/// `FinalExecutionStatus`, whose variants are `NotStarted`, `Started`,
/// `Failure` and `SuccessValue`. `SuccessReceiptId` is NOT a variant of
/// it; it only appears at the outcome level (see `OutcomeStatus`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxStatus {
    /// `SuccessValue`: the main promise chain returned a value. Necessary
    /// but not sufficient for a payment; the token receipt is checked too.
    Success,
    /// `Failure`; carries a failure summary.
    Failed(String),
    /// `NotStarted`, `Started`, or absent: not final at this milestone.
    Unknown,
    /// A shape the node never sends at the top level (for example
    /// `SuccessReceiptId`). A structural error, never success.
    Malformed(String),
}

/// Status of one transaction or receipt outcome: nearcore's
/// `ExecutionStatusView` (`Unknown`, `Failure`, `SuccessValue`,
/// `SuccessReceiptId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeStatus {
    /// Returned a value (base64, possibly empty).
    SuccessValue(String),
    /// Returned a promise: the receipt id the result is delegated to.
    SuccessReceiptId(String),
    /// Failed; carries a failure summary.
    Failure(String),
    /// Not executed yet, or an unrecognized shape.
    Unknown,
}

impl OutcomeStatus {
    /// True for both success variants. A receipt's logs are only
    /// meaningful when its own status is a success.
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            OutcomeStatus::SuccessValue(_) | OutcomeStatus::SuccessReceiptId(_)
        )
    }
}

/// One element of `receipts_outcome`: who executed the receipt, its own
/// status, the log lines it wrote, and its child receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptOutcome {
    pub id: String,
    pub executor_id: String,
    pub status: OutcomeStatus,
    pub logs: Vec<String>,
    pub receipt_ids: Vec<String>,
}

/// One element of `receipts` (only `EXPERIMENTAL_tx_status` / `tx_status`
/// return these): who sent the receipt, who receives it, and for action
/// receipts the actions it carries. `actions` is None for data receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptView {
    pub receipt_id: String,
    pub predecessor_id: String,
    pub receiver_id: String,
    pub actions: Option<Vec<TxAction>>,
}

/// One action of a transaction, limited to what the adapter inspects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxAction {
    /// A function call: method name, base64-encoded arguments, and the
    /// attached deposit in yoctoNEAR as a decimal string.
    FunctionCall {
        method_name: String,
        args_base64: String,
        deposit_yocto: String,
    },
    /// Any other action kind; opaque to the adapter.
    Other,
}

/// A transaction outcome, including its receipt tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxOutcome {
    pub tx_hash: String,
    pub signer_id: String,
    pub receiver_id: String,
    pub status: TxStatus,
    pub actions: Vec<TxAction>,
    /// `final_execution_status` (for example `FINAL`), when the node
    /// reports it.
    pub final_execution_status: Option<String>,
    /// `transaction_outcome.outcome.receipt_ids`: the roots of the
    /// receipt tree.
    pub root_receipt_ids: Vec<String>,
    /// `receipts_outcome`, in the order the node returned them.
    pub receipts_outcome: Vec<ReceiptOutcome>,
    /// `receipts` (receipt bodies). None when the method omitted them
    /// (the plain `tx` method does), in which case receipt methods and
    /// predecessors cannot be bound.
    pub receipts: Option<Vec<ReceiptView>>,
}

/// A configured JSON-RPC client. Cloneable; the underlying HTTP agent is
/// pooled and shared, and so is the learned transaction-status method.
#[derive(Clone)]
pub struct NearRpc {
    agent: ureq::Agent,
    url: String,
    timeout: Duration,
    /// Set once the node answered `tx_status` with METHOD_NOT_FOUND, so
    /// later lookups go straight to `EXPERIMENTAL_tx_status`.
    legacy_tx_status: Arc<AtomicBool>,
}

impl std::fmt::Debug for NearRpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The URL is deliberately omitted: privacy RPC endpoints embed
        // API keys in the path and must never appear in debug output.
        f.debug_struct("NearRpc")
            .field("timeout", &self.timeout)
            .field("url", &"<redacted>")
            .finish()
    }
}

impl NearRpc {
    /// Build a client for the given endpoint with a per-call timeout.
    pub fn new(endpoint: &str, timeout: Duration) -> Result<Self, RpcError> {
        let trimmed = endpoint.trim();
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err(RpcError::BadEndpoint(
                "the RPC endpoint must start with http:// or https://".to_string(),
            ));
        }
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        Ok(Self {
            agent,
            url: trimmed.to_string(),
            timeout,
            legacy_tx_status: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The configured per-call timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": "aps-near",
            "method": method,
            "params": params,
        });
        let response = self
            .agent
            .post(&self.url)
            // NEAR's RPC answers 415 unless the body is declared as JSON;
            // ureq's send_string defaults to text/plain.
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|err| RpcError::Transport(err.to_string()))?;
        let text = response
            .into_string()
            .map_err(|err| RpcError::Transport(err.to_string()))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| RpcError::Protocol(format!("response is not JSON: {err}")))?;
        if let Some(error) = value.get("error").filter(|e| !e.is_null()) {
            return Err(server_error(error));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| RpcError::Protocol("response has no result field".to_string()))
    }

    /// `status`: chain id and latest block height.
    pub fn status(&self) -> Result<StatusInfo, RpcError> {
        let result = self.call("status", json!([]))?;
        Ok(StatusInfo {
            chain_id: str_field(&result, "chain_id"),
            latest_block_height: result
                .get("sync_info")
                .and_then(|s| s.get("latest_block_height"))
                .and_then(Value::as_u64)
                .or_else(|| result.get("latest_block_height").and_then(Value::as_u64))
                .unwrap_or(0),
        })
    }

    /// Height of the latest finalized block.
    pub fn latest_final_block_height(&self) -> Result<u64, RpcError> {
        let result = self.call("block", json!({ "finality": "final" }))?;
        Ok(result
            .get("header")
            .and_then(|h| h.get("height"))
            .and_then(Value::as_u64)
            .unwrap_or(0))
    }

    /// `view_account`.
    pub fn view_account(&self, account_id: &str) -> Result<AccountView, RpcError> {
        let result = self.call(
            "query",
            json!({
                "request_type": "view_account",
                "finality": "final",
                "account_id": account_id,
            }),
        )?;
        Ok(AccountView {
            amount_yocto: str_field(&result, "amount"),
            code_hash: str_field(&result, "code_hash"),
            block_height: u64_field(&result, "block_height"),
        })
    }

    /// `view_access_key` for one public key of an account.
    pub fn view_access_key(
        &self,
        account_id: &str,
        public_key: &str,
    ) -> Result<AccessKeyView, RpcError> {
        let result = self.call(
            "query",
            json!({
                "request_type": "view_access_key",
                "finality": "final",
                "account_id": account_id,
                "public_key": public_key,
            }),
        )?;
        Ok(AccessKeyView {
            nonce: u64_field(&result, "nonce"),
            permission: result.get("permission").cloned().unwrap_or(Value::Null),
        })
    }

    /// `ft_balance_of` for one account on a NEP-141 contract. The raw
    /// balance in atomic units is returned as a decimal string.
    pub fn ft_balance_of(&self, contract: &str, account_id: &str) -> Result<String, RpcError> {
        let args = serde_json::to_string(&json!({ "account_id": account_id }))
            .map_err(|err| RpcError::Protocol(err.to_string()))?;
        let args_base64 = b64_encode(args.as_bytes());
        let result = self.call(
            "query",
            json!({
                "request_type": "call_function",
                "finality": "final",
                "account_id": contract,
                "method_name": "ft_balance_of",
                "args_base64": args_base64,
            }),
        )?;
        let decoded = decode_call_result(&result)?;
        let value: Value = serde_json::from_str(&decoded)
            .map_err(|_| RpcError::Protocol("ft_balance_of result is not JSON".to_string()))?;
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| RpcError::Protocol("ft_balance_of result is not a string".to_string()))
    }

    /// `ft_metadata` for a NEP-141 contract, as raw JSON.
    pub fn ft_metadata(&self, contract: &str) -> Result<Value, RpcError> {
        let args_base64 = b64_encode(b"{}");
        let result = self.call(
            "query",
            json!({
                "request_type": "call_function",
                "finality": "final",
                "account_id": contract,
                "method_name": "ft_metadata",
                "args_base64": args_base64,
            }),
        )?;
        let decoded = decode_call_result(&result)?;
        serde_json::from_str(&decoded)
            .map_err(|_| RpcError::Protocol("ft_metadata result is not JSON".to_string()))
    }

    /// `tx` with `wait_until: "FINAL"`. The plain `tx` method omits the
    /// receipt bodies, so its outcome has `receipts: None` and cannot
    /// confirm a payment on its own; verification uses
    /// [`NearRpc::tx_status_receipts`].
    pub fn tx_status_final(&self, tx_hash: &str, signer_id: &str) -> Result<TxOutcome, RpcError> {
        let result = self.call("tx", tx_params(tx_hash, signer_id))?;
        parse_tx_outcome(tx_hash, &result)
    }

    /// Transaction status with receipt bodies, at `wait_until: "FINAL"`.
    ///
    /// Tries `tx_status` (the nearcore 2.14 name) first and falls back to
    /// `EXPERIMENTAL_tx_status` (accepted by both 2.13 and 2.14) when the
    /// node answers METHOD_NOT_FOUND; the fallback is remembered for the
    /// lifetime of this client and its clones.
    pub fn tx_status_receipts(
        &self,
        tx_hash: &str,
        signer_id: &str,
    ) -> Result<TxOutcome, RpcError> {
        let params = tx_params(tx_hash, signer_id);
        if !self.legacy_tx_status.load(Ordering::Relaxed) {
            match self.call("tx_status", params.clone()) {
                Ok(result) => return parse_tx_outcome(tx_hash, &result),
                Err(err) if err.is_method_not_found() => {
                    self.legacy_tx_status.store(true, Ordering::Relaxed);
                }
                Err(err) => return Err(err),
            }
        }
        let result = self.call("EXPERIMENTAL_tx_status", params)?;
        parse_tx_outcome(tx_hash, &result)
    }
}

fn tx_params(tx_hash: &str, signer_id: &str) -> Value {
    json!({
        "tx_hash": tx_hash,
        "sender_account_id": signer_id,
        "wait_until": "FINAL",
    })
}

fn server_error(error: &Value) -> RpcError {
    RpcError::Server {
        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("(no message)")
            .to_string(),
        data: error
            .get("data")
            .and_then(Value::as_str)
            .map(str::to_string),
        cause: error
            .get("cause")
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Parse a transaction-status result (`tx`, `tx_status` or
/// `EXPERIMENTAL_tx_status`) into a typed outcome. Public so recorded
/// fixtures can be verified offline exactly as live responses are.
pub fn parse_tx_outcome(tx_hash: &str, result: &Value) -> Result<TxOutcome, RpcError> {
    let transaction = result.get("transaction").cloned().ok_or_else(|| {
        RpcError::Protocol("transaction result has no transaction field".to_string())
    })?;
    let status = parse_status(result.get("status"));
    let actions = parse_actions(transaction.get("actions"));
    let root_receipt_ids = result
        .get("transaction_outcome")
        .and_then(|t| t.get("outcome"))
        .map(|o| string_list(o.get("receipt_ids")))
        .unwrap_or_default();
    let receipts_outcome = result
        .get("receipts_outcome")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_receipt_outcome).collect())
        .unwrap_or_default();
    let receipts = result
        .get("receipts")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_receipt_view).collect());
    Ok(TxOutcome {
        tx_hash: tx_hash.to_string(),
        signer_id: str_field(&transaction, "signer_id"),
        receiver_id: str_field(&transaction, "receiver_id"),
        status,
        actions,
        final_execution_status: result
            .get("final_execution_status")
            .and_then(Value::as_str)
            .map(str::to_string),
        root_receipt_ids,
        receipts_outcome,
        receipts,
    })
}

/// Parse the top-level `FinalExecutionStatus`.
///
/// Only `SuccessValue` is success and only `Failure` is failure.
/// `NotStarted` / `Started` (serialized as bare strings) and an absent
/// status are not final. `SuccessReceiptId` - valid at the outcome level,
/// never at the top level - and any other shape are malformed.
fn parse_status(status: Option<&Value>) -> TxStatus {
    match status {
        None | Some(Value::Null) => TxStatus::Unknown,
        Some(Value::String(name)) if name == "NotStarted" || name == "Started" => TxStatus::Unknown,
        Some(Value::Object(map)) => {
            if map.contains_key("SuccessValue") && map.len() == 1 {
                TxStatus::Success
            } else if let Some(failure) = map.get("Failure") {
                TxStatus::Failed(failure.to_string())
            } else if map.contains_key("NotStarted") || map.contains_key("Started") {
                TxStatus::Unknown
            } else if map.contains_key("SuccessReceiptId") {
                TxStatus::Malformed("SuccessReceiptId is not a final execution status".to_string())
            } else {
                TxStatus::Malformed("unrecognized final execution status".to_string())
            }
        }
        Some(_) => TxStatus::Malformed("unrecognized final execution status".to_string()),
    }
}

/// Parse an outcome-level `ExecutionStatusView`.
fn parse_outcome_status(status: Option<&Value>) -> OutcomeStatus {
    let Some(Value::Object(map)) = status else {
        return OutcomeStatus::Unknown;
    };
    if let Some(value) = map.get("SuccessValue") {
        OutcomeStatus::SuccessValue(value.as_str().unwrap_or_default().to_string())
    } else if let Some(id) = map.get("SuccessReceiptId") {
        OutcomeStatus::SuccessReceiptId(id.as_str().unwrap_or_default().to_string())
    } else if let Some(failure) = map.get("Failure") {
        OutcomeStatus::Failure(failure.to_string())
    } else {
        OutcomeStatus::Unknown
    }
}

fn parse_receipt_outcome(value: &Value) -> ReceiptOutcome {
    let outcome = value.get("outcome").cloned().unwrap_or(Value::Null);
    ReceiptOutcome {
        id: str_field(value, "id"),
        executor_id: str_field(&outcome, "executor_id"),
        status: parse_outcome_status(outcome.get("status")),
        logs: string_list(outcome.get("logs")),
        receipt_ids: string_list(outcome.get("receipt_ids")),
    }
}

fn parse_receipt_view(value: &Value) -> ReceiptView {
    let actions = value
        .get("receipt")
        .and_then(|r| r.get("Action"))
        .map(|action| parse_actions(action.get("actions")));
    ReceiptView {
        receipt_id: str_field(value, "receipt_id"),
        predecessor_id: str_field(value, "predecessor_id"),
        receiver_id: str_field(value, "receiver_id"),
        actions,
    }
}

fn parse_actions(actions: Option<&Value>) -> Vec<TxAction> {
    actions
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_action).collect())
        .unwrap_or_default()
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_action(value: &Value) -> TxAction {
    match value.get("FunctionCall") {
        Some(call) => TxAction::FunctionCall {
            method_name: str_field(call, "method_name"),
            args_base64: str_field(call, "args"),
            deposit_yocto: str_field(call, "deposit"),
        },
        None => TxAction::Other,
    }
}

fn decode_call_result(result: &Value) -> Result<String, RpcError> {
    let bytes = result
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| RpcError::Protocol("call result is not a byte array".to_string()))?;
    let raw: Vec<u8> = bytes
        .iter()
        .filter_map(Value::as_u64)
        .map(|b| b as u8)
        .collect();
    String::from_utf8(raw).map_err(|_| RpcError::Protocol("call result is not UTF-8".to_string()))
}

fn str_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn u64_field(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or(0)
}

fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode base64 function-call arguments to a JSON value.
pub fn b64_decode_json(args_base64: &str) -> Result<Value, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(args_base64.as_bytes())
        .map_err(|err| format!("function-call arguments are not valid base64: {err}"))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "function-call arguments are not valid UTF-8".to_string())?;
    serde_json::from_str(&text)
        .map_err(|_| "function-call arguments are not valid JSON".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_requires_a_scheme() {
        assert!(NearRpc::new("1rpc.io/key/near", Duration::from_secs(5)).is_err());
        assert!(NearRpc::new("https://rpc.mainnet.near.org", Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn tx_status_shapes_are_parsed() {
        let result = json!({
            "final_execution_status": "FINAL",
            "status": { "SuccessValue": "" },
            "transaction": {
                "signer_id": "payer.near",
                "receiver_id": "usdc.near",
                "actions": [
                    { "FunctionCall": { "method_name": "ft_transfer", "args": "e30=", "gas": 1, "deposit": "1" } },
                    { "CreateAccount": {} }
                ]
            },
            "transaction_outcome": {
                "id": "txhash",
                "outcome": { "executor_id": "payer.near", "receipt_ids": ["r1"],
                             "status": { "SuccessReceiptId": "r1" }, "logs": [] }
            },
            "receipts_outcome": [
                { "id": "r1", "outcome": { "executor_id": "usdc.near", "logs": ["hello"],
                  "receipt_ids": [], "status": { "SuccessValue": "" } } }
            ],
            "receipts": [
                { "receipt_id": "r1", "predecessor_id": "payer.near", "receiver_id": "usdc.near",
                  "receipt": { "Action": { "actions": [
                      { "FunctionCall": { "method_name": "ft_transfer", "args": "e30=", "gas": 1, "deposit": "1" } }
                  ] } } }
            ]
        });
        let outcome = parse_tx_outcome("txhash", &result).expect("parses");
        assert_eq!(outcome.status, TxStatus::Success);
        assert_eq!(outcome.signer_id, "payer.near");
        assert_eq!(outcome.receiver_id, "usdc.near");
        assert_eq!(outcome.actions.len(), 2);
        assert!(matches!(outcome.actions[0], TxAction::FunctionCall { .. }));
        assert!(matches!(outcome.actions[1], TxAction::Other));
        assert_eq!(outcome.final_execution_status.as_deref(), Some("FINAL"));
        assert_eq!(outcome.root_receipt_ids, vec!["r1".to_string()]);
        assert_eq!(outcome.receipts_outcome.len(), 1);
        assert_eq!(outcome.receipts_outcome[0].executor_id, "usdc.near");
        assert_eq!(outcome.receipts_outcome[0].logs, vec!["hello".to_string()]);
        assert!(outcome.receipts_outcome[0].status.is_success());
        let receipts = outcome.receipts.expect("receipt bodies present");
        assert_eq!(receipts[0].predecessor_id, "payer.near");
        assert_eq!(receipts[0].actions.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn plain_tx_shape_has_no_receipt_bodies() {
        let result = json!({
            "status": { "SuccessValue": "" },
            "transaction": { "signer_id": "a", "receiver_id": "b", "actions": [] },
            "transaction_outcome": { "outcome": { "receipt_ids": [] } },
            "receipts_outcome": []
        });
        let outcome = parse_tx_outcome("h", &result).expect("parses");
        assert!(outcome.receipts.is_none());
        assert!(outcome.final_execution_status.is_none());
    }

    #[test]
    fn final_status_accepts_only_success_value_and_failure() {
        assert_eq!(
            parse_status(Some(&json!({ "SuccessValue": "" }))),
            TxStatus::Success
        );
        let failed = parse_status(Some(&json!({ "Failure": { "error": "x" } })));
        assert!(matches!(failed, TxStatus::Failed(_)));
        // SuccessReceiptId is an outcome-level status only; at the top level
        // it is a structural error, never success.
        assert!(matches!(
            parse_status(Some(&json!({ "SuccessReceiptId": "r1" }))),
            TxStatus::Malformed(_)
        ));
        assert!(matches!(
            parse_status(Some(&json!({ "Unknown": {} }))),
            TxStatus::Malformed(_)
        ));
        assert!(matches!(
            parse_status(Some(&json!(42))),
            TxStatus::Malformed(_)
        ));
        // NotStarted / Started (bare strings in nearcore's serde) and an
        // absent status are "not final yet".
        assert_eq!(parse_status(Some(&json!("NotStarted"))), TxStatus::Unknown);
        assert_eq!(parse_status(Some(&json!("Started"))), TxStatus::Unknown);
        assert_eq!(parse_status(None), TxStatus::Unknown);
    }

    #[test]
    fn outcome_status_keeps_success_receipt_id() {
        assert_eq!(
            parse_outcome_status(Some(&json!({ "SuccessReceiptId": "r9" }))),
            OutcomeStatus::SuccessReceiptId("r9".to_string())
        );
        assert!(parse_outcome_status(Some(&json!({ "SuccessReceiptId": "r9" }))).is_success());
        assert!(!parse_outcome_status(Some(&json!({ "Failure": {} }))).is_success());
        assert_eq!(parse_outcome_status(None), OutcomeStatus::Unknown);
    }

    fn server(code: i64, message: &str, data: Option<&str>, cause: Option<&str>) -> RpcError {
        RpcError::Server {
            code,
            message: message.to_string(),
            data: data.map(str::to_string),
            cause: cause.map(str::to_string),
        }
    }

    #[test]
    fn not_found_detection_covers_message_and_data() {
        assert!(server(-32000, "Server error", Some("Tx 6zgh not found"), None).is_not_found());
        assert!(server(-32000, "account does not exist while viewing", None, None).is_not_found());
        assert!(server(-32000, "Server error", None, Some("UNKNOWN_TRANSACTION")).is_not_found());
        assert!(!server(-32000, "gas limit exceeded", None, None).is_not_found());
    }

    #[test]
    fn error_classes_from_recorded_node_errors() {
        // Shapes recorded from rpc.mainnet.near.org (nearcore 2.14.0) on
        // 2026-11-04.
        let timeout = server_error(&json!({
            "name": "HANDLER_ERROR",
            "cause": { "info": { "cause": "NOT_OBSERVED" }, "name": "TIMEOUT_ERROR" },
            "code": -32000, "message": "Server error", "data": "Timeout"
        }));
        assert!(timeout.is_timeout());
        assert!(!timeout.is_method_not_found());
        let missing = server_error(&json!({
            "name": "REQUEST_VALIDATION_ERROR",
            "cause": { "name": "METHOD_NOT_FOUND", "info": { "method_name": "tx_status" } },
            "code": -32601, "message": "Method not found", "data": "tx_status"
        }));
        assert!(missing.is_method_not_found());
        assert!(!missing.is_timeout());
        assert!(!missing.is_not_found());
    }

    #[test]
    fn base64_json_round_trip() {
        let value = json!({ "receiver_id": "gateway.near", "amount": "1000000" });
        let encoded = b64_encode(value.to_string().as_bytes());
        let decoded = b64_decode_json(&encoded).expect("decodes");
        assert_eq!(decoded, value);
        assert!(b64_decode_json("!!!not-base64!!!").is_err());
    }
}
