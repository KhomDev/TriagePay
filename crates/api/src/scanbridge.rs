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
//! - `APS_SCAN_SKU`: `scan.v1` (default; deterministic gates only) or
//!   `triage.v2` (the gates plus the Stage B model judge). The scanner
//!   subprocess inherits the environment, so `APS_LIVE_MODEL` and
//!   `NEARAI_API_KEY` reach the judge; without them Stage B reports
//!   "unavailable" and the verdict stays at Stage A's or lower.

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
    sku: &'static str,
}

/// The SKUs the scanner CLI accepts.
pub const SCANNER_SKUS: [&str; 2] = [aps_core::SCAN_SKU, aps_core::TRIAGE_V2_SKU];

/// Resolve `APS_SCAN_SKU` to a supported SKU; `None` when it is set to
/// anything else.
pub fn sku_from_env() -> Option<&'static str> {
    match std::env::var("APS_SCAN_SKU") {
        Err(_) => Some(aps_core::SCAN_SKU),
        Ok(raw) if raw.trim().is_empty() => Some(aps_core::SCAN_SKU),
        Ok(raw) => SCANNER_SKUS.iter().copied().find(|sku| *sku == raw.trim()),
    }
}

impl ScanLauncher {
    /// Build a launcher with explicit settings (tests, demos).
    pub fn new(python: impl Into<String>, scanner_dir: PathBuf, timeout: Duration) -> Self {
        ScanLauncher {
            python: python.into(),
            scanner_dir,
            timeout,
            sku: aps_core::SCAN_SKU,
        }
    }

    /// The same launcher running another supported SKU.
    pub fn with_sku(mut self, sku: &'static str) -> Self {
        self.sku = sku;
        self
    }

    /// The SKU every scan runs (and every paid requirement names).
    pub fn sku(&self) -> &'static str {
        self.sku
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
            sku: sku_from_env().unwrap_or(aps_core::SCAN_SKU),
        }
    }

    /// Run one scan.
    ///
    /// The envelope JSON is forwarded verbatim: the scanner is the
    /// schema authority. This must be called without a ledger lock held
    /// so a slow scan never blocks other accounts.
    pub async fn launch(&self, envelope_json: &str) -> Result<ScanOutcome, ScanBridgeError> {
        let mut command = Command::new(&self.python);
        command
            .arg("-m")
            .arg("aps_scanner")
            .arg("--sku")
            .arg(self.sku)
            .current_dir(&self.scanner_dir);
        // The scanner runs in its own directory, so a relative path in one
        // of these variables would resolve against the wrong folder and the
        // judge would fail closed without saying why. Resolve them against
        // this process's working directory first.
        for (name, path) in relative_path_overrides() {
            command.env(name, path);
        }
        // The model judge must give up before this process is killed, so
        // a slow Stage B call fails closed (needs_evidence) and the scan
        // still produces a report. Five seconds cover start-up and Stage A.
        command.env(STAGE_B_BUDGET_ENV, stage_b_budget(self.timeout).to_string());
        let mut child = command
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

/// Seconds the model judge may spend, retries included (read by
/// triage_model.judge).
pub const STAGE_B_BUDGET_ENV: &str = "APS_TRIAGE_BUDGET_SECS";

/// The Stage B budget for a scanner deadline: strictly below it.
pub fn stage_b_budget(timeout: Duration) -> u64 {
    timeout.as_secs().saturating_sub(5).max(1)
}

/// Path variables the scanner and the model judge read.
pub const PATH_VARIABLES: [&str; 3] = [
    "APS_TRIAGE_CASSETTES",
    "APS_TRIAGE_CACHE_DIR",
    "APS_TRIAGE_MODEL_DIR",
];

/// Absolute values for the path variables that are set to a relative path,
/// resolved against the current working directory.
pub fn relative_path_overrides() -> Vec<(&'static str, PathBuf)> {
    let Ok(cwd) = std::env::current_dir() else {
        return Vec::new();
    };
    PATH_VARIABLES
        .iter()
        .filter_map(|name| {
            let value = std::env::var(name).ok()?;
            let value = value.trim();
            if value.is_empty() {
                return None;
            }
            let path = PathBuf::from(value);
            (!path.is_absolute()).then(|| (*name, cwd.join(path)))
        })
        .collect()
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
