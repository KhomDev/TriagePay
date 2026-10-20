//! A tiny deterministic JSON-RPC stub server for offline adapter tests.
//!
//! Listens on an ephemeral loopback port, answers every JSON-RPC POST by
//! dispatching to a canned handler, records each request for assertions,
//! and closes the connection after each response. No network access
//! beyond loopback; no dependencies outside the standard library.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

/// A canned-response handler: receives the full JSON-RPC request and
/// returns either the result object or a JSON-RPC error object.
pub type Handler = Arc<dyn Fn(&Value) -> Result<Value, Value> + Send + Sync>;

/// A running stub server.
pub struct StubRpc {
    /// The endpoint URL, for example http://127.0.0.1:39471
    pub url: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl StubRpc {
    /// Start a stub server with the given handler.
    pub fn start(handler: Handler) -> StubRpc {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let requests: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let recorded = Arc::clone(&recorded);
                let handler = Arc::clone(&handler);
                thread::spawn(move || handle_one(stream, handler, recorded));
            }
        });
        StubRpc {
            url: format!("http://127.0.0.1:{port}"),
            requests,
        }
    }

    /// Every JSON-RPC request body the server has received, in order.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

fn handle_one(mut stream: TcpStream, handler: Handler, recorded: Arc<Mutex<Vec<Value>>>) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(header_end) = find_header_end(&buffer) {
                    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                    let content_length = header
                        .lines()
                        .find_map(|line| {
                            let lower = line.to_ascii_lowercase();
                            if lower.starts_with("content-length:") {
                                line.split(':')
                                    .nth(1)
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if buffer.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    let body = String::from_utf8_lossy(&buffer).to_string();
    let json_body = body.split("\r\n\r\n").nth(1).unwrap_or_default();
    let request: Value = match serde_json::from_str(json_body) {
        Ok(value) => value,
        Err(_) => return,
    };
    if let Ok(mut recorded) = recorded.lock() {
        recorded.push(request.clone());
    }
    let response = match handler(&request) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": "aps-near", "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": "aps-near", "error": error}),
    };
    let text = response.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        text.len(),
        text
    );
    let _ = stream.flush();
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// The JSON-RPC error a NEAR node returns for an unknown transaction.
pub fn tx_not_found_error() -> Value {
    json!({
        "code": -32000,
        "message": "Server error",
        "data": "Tx 6zgh2u9DqHHiXzdy9ouTP7oGky2T4nugbhqsK7wQHnZS not found"
    })
}

/// A finalized, successful ft_transfer outcome for the stub server.
#[allow(clippy::too_many_arguments)]
pub fn successful_ft_transfer_tx(
    tx_hash: &str,
    signer: &str,
    asset_contract: &str,
    gateway: &str,
    memo: &str,
    amount_atomic: &str,
    deposit_yocto: &str,
) -> Value {
    let args = json!({
        "receiver_id": gateway,
        "amount": amount_atomic,
        "memo": memo,
    });
    let args_base64 = b64_encode(args.to_string().as_bytes());
    json!({
        "status": { "SuccessValue": "" },
        "transaction": {
            "signer_id": signer,
            "receiver_id": asset_contract,
            "actions": [
                { "FunctionCall": {
                    "method_name": "ft_transfer",
                    "args": args_base64,
                    "gas": 30000000000000u64,
                    "deposit": deposit_yocto
                } }
            ],
            "nonce": 7,
            "hash": tx_hash
        },
        "transaction_outcome": {
            "id": tx_hash,
            "block_hash": "blockhash",
            "outcome": { "status": { "SuccessReceiptId": "receipt1" } }
        },
        "receipts_outcome": []
    })
}

/// A finalized, failed transaction outcome.
pub fn failed_tx(tx_hash: &str, signer: &str, receiver: &str) -> Value {
    json!({
        "status": { "Failure": { "Action #0": { "kind": "function_call_error" } } },
        "transaction": {
            "signer_id": signer,
            "receiver_id": receiver,
            "actions": [ { "CreateAccount": {} } ],
            "nonce": 7,
            "hash": tx_hash
        },
        "transaction_outcome": { "id": tx_hash, "outcome": {} },
        "receipts_outcome": []
    })
}

fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
