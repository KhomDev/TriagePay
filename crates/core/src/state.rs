//! Job lifecycle states, identifiers, and report hashes.

use crate::error::CoreError;
use std::fmt;

/// Opaque job identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(String);

impl JobId {
    /// Construct a job identifier from its internal string.
    pub(crate) fn new(id: String) -> Self {
        JobId(id)
    }

    /// The identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A 32-byte content hash binding a settlement to its report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReportHash(pub [u8; 32]);

impl ReportHash {
    /// True when every byte is zero. The ledger rejects zero hashes so a
    /// settlement always binds to real report content.
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }

    /// Lowercase hexadecimal rendering, 64 characters.
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push_str(&format!("{:02x}", byte));
        }
        out
    }

    /// Parse 64 hexadecimal characters (either case).
    pub fn from_hex(s: &str) -> Result<Self, CoreError> {
        let err = || CoreError::Validation("report hash must be 64 hex characters".to_string());
        if s.len() != 64 {
            return Err(err());
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = hex_val(chunk[0]).ok_or_else(err)?;
            let lo = hex_val(chunk[1]).ok_or_else(err)?;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(ReportHash(bytes))
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Lifecycle of a paid job.
///
/// Legal transitions: Reserved to Running or Refunded; Running to Settled
/// or Refunded. Settled and Refunded are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Funds reserved, work not yet started.
    Reserved,
    /// Work in progress.
    Running,
    /// Work completed; reserved credit consumed.
    Settled,
    /// Reservation returned to available credit.
    Refunded,
}

impl JobState {
    /// Terminal states accept no further transitions.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Settled | JobState::Refunded)
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = match self {
            JobState::Reserved => "reserved",
            JobState::Running => "running",
            JobState::Settled => "settled",
            JobState::Refunded => "refunded",
        };
        f.write_str(name)
    }
}
