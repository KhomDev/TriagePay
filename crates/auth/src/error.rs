//! Authentication failures.
//!
//! Messages are safe to return to a client: they never echo a key, a
//! signature, or a nonce, and API-key failures do not reveal whether a
//! key was unknown or revoked.

use std::fmt;

/// Every authentication failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// The API key is not `aps_` followed by 64 lowercase hex characters.
    ApiKeyMalformed,
    /// The API key is unknown or its caller is revoked.
    ApiKeyRejected,
    /// The OS random source failed.
    Random(String),
    /// The caller registry failed.
    Store(String),
    /// A request field is missing or malformed.
    Malformed(String),
    /// The public key is not an ed25519 key.
    UnsupportedKeyType,
    /// The timestamp is older than the window allows.
    TimestampExpired,
    /// The timestamp is further in the future than the skew allows.
    TimestampInFuture,
    /// The ed25519 signature does not verify over the rebuilt payload.
    BadSignature,
    /// The key is not an access key of the claimed account.
    KeyNotFound,
    /// The key exists but is a function-call key; NEP-413 requires a
    /// full-access key.
    KeyNotFullAccess,
    /// The claimed account does not exist.
    UnknownAccount,
    /// The nonce was already used.
    NonceReplayed,
    /// The key lookup (RPC) failed; the request may be retried.
    LookupUnavailable(String),
    /// The account has more access-key pages than the lookup will read.
    TooManyKeyPages,
}

impl AuthError {
    /// The HTTP status the API should answer with.
    pub fn http_status(&self) -> u16 {
        match self {
            AuthError::Random(_) | AuthError::Store(_) => 500,
            AuthError::LookupUnavailable(_) | AuthError::TooManyKeyPages => 503,
            AuthError::Malformed(_) | AuthError::UnsupportedKeyType => 400,
            AuthError::KeyNotFullAccess => 403,
            _ => 401,
        }
    }

    /// Stable machine-readable code for the error body.
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::ApiKeyMalformed | AuthError::ApiKeyRejected => "unauthenticated",
            AuthError::Random(_) | AuthError::Store(_) => "internal",
            AuthError::Malformed(_) => "malformed_auth",
            AuthError::UnsupportedKeyType => "unsupported_key_type",
            AuthError::TimestampExpired | AuthError::TimestampInFuture => "stale_request",
            AuthError::BadSignature => "bad_signature",
            AuthError::KeyNotFound | AuthError::UnknownAccount => "key_not_on_account",
            AuthError::KeyNotFullAccess => "full_access_key_required",
            AuthError::NonceReplayed => "nonce_replayed",
            AuthError::LookupUnavailable(_) | AuthError::TooManyKeyPages => {
                "key_lookup_unavailable"
            }
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AuthError::ApiKeyMalformed => write!(f, "API key is malformed"),
            AuthError::ApiKeyRejected => write!(f, "API key is not valid"),
            AuthError::Random(msg) => write!(f, "random source failed: {msg}"),
            AuthError::Store(msg) => write!(f, "caller registry failed: {msg}"),
            AuthError::Malformed(msg) => write!(f, "malformed signed request: {msg}"),
            AuthError::UnsupportedKeyType => write!(f, "only ed25519 keys are supported"),
            AuthError::TimestampExpired => write!(f, "request timestamp is too old"),
            AuthError::TimestampInFuture => write!(f, "request timestamp is in the future"),
            AuthError::BadSignature => write!(f, "signature does not verify"),
            AuthError::KeyNotFound => write!(f, "key is not an access key of the account"),
            AuthError::KeyNotFullAccess => {
                write!(
                    f,
                    "NEP-413 requires a full-access key; use an API key instead"
                )
            }
            AuthError::UnknownAccount => write!(f, "account does not exist"),
            AuthError::NonceReplayed => write!(f, "nonce was already used"),
            AuthError::LookupUnavailable(msg) => write!(f, "key lookup unavailable: {msg}"),
            AuthError::TooManyKeyPages => write!(f, "account has too many access keys to scan"),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<aps_store::StoreError> for AuthError {
    fn from(err: aps_store::StoreError) -> Self {
        AuthError::Store(err.to_string())
    }
}
