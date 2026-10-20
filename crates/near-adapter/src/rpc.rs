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
    /// The node answered with a JSON-RPC error object.
    Server {
        code: i64,
        message: String,
        data: Option<String>,
    },
}

impl RpcError {
    /// True when the node reported a not-found condition for a query or
    /// transaction. The provider translates "not there (yet)" into a
    /// pending observation instead of surfacing an error.
    pub fn is_not_found(&self) -> bool {
        match self {
            RpcError::Server { message, data, .. } => {
                let hay = format!("{message} {}", data.as_deref().unwrap_or(""));
                let lowered = hay.to_ascii_lowercase();
                lowered.contains("not found") || lowered.contains("does not exist")
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

/// Execution status of a transaction, as reported by the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxStatus {
    /// Fully successful execution (`SuccessValue` or `SuccessReceiptId`).
    Success,
    /// Execution failed; carries a failure summary.
    Failed(String),
    /// Not final or unknown at the requested milestone.
    Unknown,
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

/// A finalized transaction outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxOutcome {
    pub tx_hash: String,
    pub signer_id: String,
    pub receiver_id: String,
    pub status: TxStatus,
    pub actions: Vec<TxAction>,
}

/// A configured JSON-RPC client. Cloneable; the underlying HTTP agent is
/// pooled and shared.
#[derive(Clone)]
pub struct NearRpc {
    agent: ureq::Agent,
    url: String,
    timeout: Duration,
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
            return Err(RpcError::Server {
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
            });
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

    /// `tx` with `wait_until: "FINAL"`: the outcome of a transaction
    /// once its block and receipts are finalized.
    pub fn tx_status_final(&self, tx_hash: &str, signer_id: &str) -> Result<TxOutcome, RpcError> {
        let result = self.call(
            "tx",
            json!({
                "tx_hash": tx_hash,
                "sender_account_id": signer_id,
                "wait_until": "FINAL",
            }),
        )?;
        parse_tx_outcome(tx_hash, &result)
    }
}

fn parse_tx_outcome(tx_hash: &str, result: &Value) -> Result<TxOutcome, RpcError> {
    let transaction = result.get("transaction").cloned().ok_or_else(|| {
        RpcError::Protocol("transaction result has no transaction field".to_string())
    })?;
    let status = parse_status(result.get("status"));
    let actions = transaction
        .get("actions")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_action).collect())
        .unwrap_or_default();
    Ok(TxOutcome {
        tx_hash: tx_hash.to_string(),
        signer_id: str_field(&transaction, "signer_id"),
        receiver_id: str_field(&transaction, "receiver_id"),
        status,
        actions,
    })
}

fn parse_status(status: Option<&Value>) -> TxStatus {
    let map = match status {
        Some(Value::Object(map)) => map,
        _ => return TxStatus::Unknown,
    };
    if map.contains_key("SuccessValue") || map.contains_key("SuccessReceiptId") {
        TxStatus::Success
    } else if let Some(failure) = map.get("Failure") {
        TxStatus::Failed(failure.to_string())
    } else {
        TxStatus::Unknown
    }
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
pub(crate) fn b64_decode_json(args_base64: &str) -> Result<Value, String> {
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
            "status": { "SuccessReceiptId": "r1" },
            "transaction": {
                "signer_id": "payer.near",
                "receiver_id": "usdc.near",
                "actions": [
                    { "FunctionCall": { "method_name": "ft_transfer", "args": "e30=", "gas": 1, "deposit": "1" } },
                    { "CreateAccount": {} }
                ]
            }
        });
        let outcome = parse_tx_outcome("txhash", &result).expect("parses");
        assert_eq!(outcome.status, TxStatus::Success);
        assert_eq!(outcome.signer_id, "payer.near");
        assert_eq!(outcome.receiver_id, "usdc.near");
        assert_eq!(outcome.actions.len(), 2);
        assert!(matches!(outcome.actions[0], TxAction::FunctionCall { .. }));
        assert!(matches!(outcome.actions[1], TxAction::Other));
    }

    #[test]
    fn failure_and_unknown_statuses_map_cleanly() {
        let failed = parse_status(Some(&json!({ "Failure": { "error": "x" } })));
        assert!(matches!(failed, TxStatus::Failed(_)));
        assert_eq!(
            parse_status(Some(&json!({ "Unknown": {} }))),
            TxStatus::Unknown
        );
        assert_eq!(parse_status(None), TxStatus::Unknown);
    }

    #[test]
    fn not_found_detection_covers_message_and_data() {
        let err = RpcError::Server {
            code: -32000,
            message: "Server error".to_string(),
            data: Some("Tx 6zgh not found".to_string()),
        };
        assert!(err.is_not_found());
        let missing = RpcError::Server {
            code: -32000,
            message: "account does not exist while viewing".to_string(),
            data: None,
        };
        assert!(missing.is_not_found());
        let other = RpcError::Server {
            code: -32000,
            message: "gas limit exceeded".to_string(),
            data: None,
        };
        assert!(!other.is_not_found());
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
