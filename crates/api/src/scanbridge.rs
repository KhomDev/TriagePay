//! Subprocess bridge to the Python scan engine.
//!
//! The scanner runs as `<python> -m aps_scanner` with the envelope JSON
//! on stdin and the public report JSON on stdout. Process isolation is a
//! security boundary: hostile finding text never enters this process.
//!
//! Configuration (environment variables, all optional):
//! - `APS_SCANNER_PYTHON`: interpreter to use (default: discovered);
//! - `APS_SCANNER_DIR`: directory containing the `aps_scanner` package
//!   (default: discovered relative to the working directory);
//! - `APS_SCANNER_TIMEOUT_SECS`: hard per-scan timeout (default: 30).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// A configured scanner launcher.
#[derive(Debug, Clone)]
pub struct ScanLauncher {
    python: String,
    scanner_dir: PathBuf,
    timeout: Duration,
}

impl ScanLauncher {
    /// Build a launcher with explicit settings (tests, demos).
    pub fn new(python: impl Into<String>, scanner_dir: PathBuf, timeout: Duration) -> Self {
        ScanLauncher {
            python: python.into(),
            scanner_dir,
            timeout,
        }
    }

    /// Discover settings from the environment with safe defaults.
    pub fn discover() -> Self {
        let python = std::env::var("APS_SCANNER_PYTHON").unwrap_or_else(|_| default_python());
        let scanner_dir = std::env::var("APS_SCANNER_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_scanner_dir());
        let timeout = Duration::from_secs(
            std::env::var("APS_SCANNER_TIMEOUT_SECS")
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok())
                .unwrap_or(30),
        );
        ScanLauncher {
            python,
            scanner_dir,
            timeout,
        }
    }

    /// Run one scan.
    ///
    /// The envelope JSON is forwarded verbatim: the scanner is the
    /// schema authority. This must be called without a ledger lock held
    /// so a slow scan never blocks other accounts.
    pub async fn launch(&self, envelope_json: &str) -> Result<ScanOutcome, ScanBridgeError> {
        let mut child = Command::new(&self.python)
            .arg("-m")
            .arg("aps_scanner")
            .current_dir(&self.scanner_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // A timed-out scanner is killed when its future is dropped.
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| ScanBridgeError::Launch(format!("cannot start scanner: {err}")))?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| ScanBridgeError::Launch("scanner stdin unavailable".to_string()))?;
            stdin
                .write_all(envelope_json.as_bytes())
                .await
                .map_err(|err| {
                    ScanBridgeError::Launch(format!("cannot write scanner stdin: {err}"))
                })?;
            // stdin drops here: the scanner sees EOF and answers.
        }
        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Err(_) => {
                return Err(ScanBridgeError::Timeout {
                    seconds: self.timeout.as_secs(),
                })
            }
            Ok(result) => result,
        }
        .map_err(|err| ScanBridgeError::Launch(format!("scanner did not run: {err}")))?;
        parse_output(output.status, &output.stdout, &output.stderr)
    }
}

fn default_python() -> String {
    for candidate in ["python3", "python"] {
        let probe = std::process::Command::new(candidate)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        if let Ok(out) = probe {
            if out.status.success() {
                return candidate.to_string();
            }
        }
    }
    "python3".to_string()
}

fn default_scanner_dir() -> PathBuf {
    let candidates = ["scanner", "../scanner", "../../scanner", "../../../scanner"];
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.join("aps_scanner").join("__main__.py").exists() {
            return path;
        }
    }
    PathBuf::from("scanner")
}

fn parse_output(
    status: std::process::ExitStatus,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<ScanOutcome, ScanBridgeError> {
    let code = status.code();
    let stdout_text = String::from_utf8_lossy(stdout);
    match code {
        Some(0) => {
            let report: Value = serde_json::from_str(&stdout_text).map_err(|err| {
                ScanBridgeError::MalformedOutput(format!("scanner stdout is not JSON: {err}"))
            })?;
            let hash = report
                .get("content_hash")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    ScanBridgeError::MalformedOutput("report is missing content_hash".to_string())
                })?;
            if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ScanBridgeError::MalformedOutput(
                    "content_hash must be 64 hex characters".to_string(),
                ));
            }
            Ok(ScanOutcome::Report {
                report,
                content_hash: hash.to_string(),
            })
        }
        Some(2) => {
            let payload: Value = serde_json::from_str(&stdout_text).map_err(|err| {
                ScanBridgeError::MalformedOutput(format!("scanner error output is not JSON: {err}"))
            })?;
            let errors = payload
                .get("errors")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(ScanOutcome::InvalidInput { errors })
        }
        other => Err(ScanBridgeError::ScannerFailed {
            exit_code: other,
            stderr: String::from_utf8_lossy(stderr).trim().to_string(),
        }),
    }
}

/// What one scanner run produced.
#[derive(Debug)]
pub enum ScanOutcome {
    /// A report was produced; payment covers the scan, not the verdict.
    Report { report: Value, content_hash: String },
    /// The envelope failed validation; the job must not be charged.
    InvalidInput { errors: Vec<String> },
}

/// Bridge failures. All map to 500 at the API: the reservation stays
/// Running and is refundable.
#[derive(Debug)]
pub enum ScanBridgeError {
    Launch(String),
    Timeout {
        seconds: u64,
    },
    MalformedOutput(String),
    ScannerFailed {
        exit_code: Option<i32>,
        stderr: String,
    },
}
