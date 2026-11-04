//! Journal event bodies.
//!
//! Bodies are serialized once with serde_json and stored verbatim; the
//! hash chain covers the stored bytes, so no re-canonicalization is ever
//! needed. Money crosses the journal as decimal strings of atomic units.

use serde::{Deserialize, Serialize};

/// One journaled mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LedgerEvent {
    /// Credit deposited into an account (creates it on first deposit).
    Deposit {
        caller: String,
        account: String,
        #[serde(with = "atomic")]
        amount: u128,
        idempotency_key: String,
    },
    /// Credit reserved for a job. `job_id` is the id the core assigned.
    Reserve {
        caller: String,
        account: String,
        sku: String,
        #[serde(with = "atomic")]
        amount: u128,
        idempotency_key: String,
        job_id: String,
    },
    /// A job moved from reserved to running.
    Start { job_id: String },
    /// A running job settled against a report hash (64 hex characters).
    Settle { job_id: String, report_hash: String },
    /// A reserved or running job refunded.
    Refund { job_id: String, reason: String },
    /// A payment intent changed state. Ledger balances are unaffected;
    /// the funding deposit is its own `Deposit` event.
    IntentState {
        intent_id: String,
        account: String,
        state: String,
        detail: Option<String>,
    },
    /// An API-level idempotency record: the response the API returned for
    /// a request, keyed by caller, scope and client key.
    IdempotencyRecord {
        caller: String,
        scope: String,
        key: String,
        request_sha256: String,
        response: String,
    },
}

impl LedgerEvent {
    /// Stable kind label stored beside the body and covered by the hash.
    pub fn kind(&self) -> &'static str {
        match self {
            LedgerEvent::Deposit { .. } => "deposit",
            LedgerEvent::Reserve { .. } => "reserve",
            LedgerEvent::Start { .. } => "start",
            LedgerEvent::Settle { .. } => "settle",
            LedgerEvent::Refund { .. } => "refund",
            LedgerEvent::IntentState { .. } => "intent_state",
            LedgerEvent::IdempotencyRecord { .. } => "idempotency_record",
        }
    }

    /// Serialize the body for storage.
    pub fn to_body(&self) -> Result<String, crate::StoreError> {
        serde_json::to_string(self)
            .map_err(|err| crate::StoreError::Validation(format!("cannot encode event: {err}")))
    }

    /// Decode a stored body and check it against its stored kind label.
    pub fn from_body(seq: u64, kind: &str, body: &str) -> Result<Self, crate::StoreError> {
        let event: LedgerEvent =
            serde_json::from_str(body).map_err(|err| crate::StoreError::Corrupt {
                seq,
                reason: format!("undecodable body: {err}"),
            })?;
        if event.kind() != kind {
            return Err(crate::StoreError::Corrupt {
                seq,
                reason: format!("kind label {kind:?} does not match body {:?}", event.kind()),
            });
        }
        Ok(event)
    }
}

/// u128 atomic units as a decimal string.
pub(crate) mod atomic {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let raw = String::deserialize(deserializer)?;
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return Err(serde::de::Error::custom(
                "atomic amount must be decimal digits",
            ));
        }
        raw.parse::<u128>()
            .map_err(|_| serde::de::Error::custom("atomic amount overflows u128"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_round_trips_and_keeps_amounts_as_strings() {
        let event = LedgerEvent::Deposit {
            caller: "c".into(),
            account: "a".into(),
            amount: u128::MAX,
            idempotency_key: "k".into(),
        };
        let body = event.to_body().unwrap();
        assert!(body.contains(&format!("\"{}\"", u128::MAX)));
        assert_eq!(LedgerEvent::from_body(1, "deposit", &body).unwrap(), event);
    }

    #[test]
    fn kind_label_must_match_body() {
        let body = LedgerEvent::Start {
            job_id: "job_1".into(),
        }
        .to_body()
        .unwrap();
        assert!(matches!(
            LedgerEvent::from_body(7, "refund", &body),
            Err(crate::StoreError::Corrupt { seq: 7, .. })
        ));
    }

    #[test]
    fn negative_or_textual_amounts_are_rejected() {
        let body =
            r#"{"type":"deposit","caller":"c","account":"a","amount":"-1","idempotency_key":"k"}"#;
        assert!(LedgerEvent::from_body(1, "deposit", body).is_err());
        let body =
            r#"{"type":"deposit","caller":"c","account":"a","amount":5,"idempotency_key":"k"}"#;
        assert!(LedgerEvent::from_body(1, "deposit", body).is_err());
    }
}
