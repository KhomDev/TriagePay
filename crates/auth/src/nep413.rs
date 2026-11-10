//! NEP-413 signed requests: "your NEAR account is your API identity".
//!
//! Source: NEP-413, Status Final,
//! <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>, accessed
//! 2026-10-21, as recorded in `docs/research/nep413-signed-messages.md`.
//!
//! Signed bytes: `SHA-256(borsh(u32 2^31 + 413) || borsh(Payload))`, where
//! `Payload = { message: String, nonce: [u8; 32], recipient: String,
//! callbackUrl: Option<String> }` in that order. The wallet signs the
//! 32-byte hash with an ed25519 **full-access** key.
//!
//! Server-side profile (see `docs/operations/auth.md`):
//! - `recipient` is the service's fixed identifier, never client input;
//! - `message` is the canonical JSON of `{scheme, method, path,
//!   body_sha256, timestamp_ms}`, rebuilt from server-side values;
//! - `callbackUrl` is none;
//! - the key must be a full-access key of the claimed account, checked
//!   through an injected [`AccessKeyLookup`];
//! - the timestamp must sit inside a window, and the nonce is single use.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::Serialize;

use crate::error::AuthError;

/// The NEP-413 tag, 2^31 + 413.
pub const NEP413_TAG: u32 = 2_147_484_061;

/// The `scheme` field of the canonical request message.
pub const MESSAGE_SCHEME: &str = "aps-nep413-request-v1";

/// Borsh bytes of the NEP-413 payload, prefixed with the tag.
pub fn payload_bytes(
    message: &str,
    nonce: &[u8; 32],
    recipient: &str,
    callback_url: Option<&str>,
) -> Vec<u8> {
    fn borsh_string(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }
    let mut out = Vec::with_capacity(4 + 4 + message.len() + 32 + 4 + recipient.len() + 1);
    out.extend_from_slice(&NEP413_TAG.to_le_bytes());
    borsh_string(&mut out, message);
    out.extend_from_slice(nonce);
    borsh_string(&mut out, recipient);
    match callback_url {
        None => out.push(0),
        Some(url) => {
            out.push(1);
            borsh_string(&mut out, url);
        }
    }
    out
}

/// The 32-byte hash a NEP-413 wallet signs.
pub fn payload_hash(
    message: &str,
    nonce: &[u8; 32],
    recipient: &str,
    callback_url: Option<&str>,
) -> [u8; 32] {
    aps_store::sha256(&payload_bytes(message, nonce, recipient, callback_url))
}

#[derive(Serialize)]
struct CanonicalRequest<'a> {
    scheme: &'a str,
    method: &'a str,
    path: &'a str,
    body_sha256: &'a str,
    timestamp_ms: u64,
}

/// The canonical message for an HTTP request. A browser client produces
/// the same bytes with `JSON.stringify({scheme, method, path,
/// body_sha256, timestamp_ms})` in that key order.
pub fn canonical_message(
    method: &str,
    path: &str,
    body: &[u8],
    timestamp_ms: u64,
) -> Result<String, AuthError> {
    validate_method(method)?;
    validate_path(path)?;
    let body_sha256 = aps_store::to_hex(&aps_store::sha256(body));
    serde_json::to_string(&CanonicalRequest {
        scheme: MESSAGE_SCHEME,
        method,
        path,
        body_sha256: &body_sha256,
        timestamp_ms,
    })
    .map_err(|err| AuthError::Malformed(format!("cannot encode message: {err}")))
}

fn validate_method(method: &str) -> Result<(), AuthError> {
    if method.is_empty() || method.len() > 10 || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(AuthError::Malformed(
            "method must be uppercase letters".into(),
        ));
    }
    Ok(())
}

/// Path and query as received: printable ASCII, no quote or backslash,
/// so the canonical JSON is the same in every JSON encoder.
fn validate_path(path: &str) -> Result<(), AuthError> {
    let ok = path.starts_with('/')
        && path.len() <= 2048
        && path
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b'\\');
    if ok {
        Ok(())
    } else {
        Err(AuthError::Malformed(
            "path must be printable ASCII starting with /".into(),
        ))
    }
}

/// NEAR account id rules: 2 to 64 characters, dot-separated parts of
/// `[a-z0-9]` joined by single `-` or `_`.
pub fn is_valid_account_id(account_id: &str) -> bool {
    if account_id.len() < 2 || account_id.len() > 64 {
        return false;
    }
    account_id.split('.').all(|part| {
        let bytes = part.as_bytes();
        !bytes.is_empty()
            && bytes[0].is_ascii_alphanumeric()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
            && !bytes
                .windows(2)
                .any(|w| matches!(w[0], b'-' | b'_') && matches!(w[1], b'-' | b'_'))
    })
}

/// Parse `ed25519:<base58>` into a verifying key.
pub fn parse_public_key(text: &str) -> Result<VerifyingKey, AuthError> {
    let encoded = match text.split_once(':') {
        Some(("ed25519", rest)) => rest,
        Some(_) => return Err(AuthError::UnsupportedKeyType),
        None => {
            return Err(AuthError::Malformed(
                "public key needs a type prefix".into(),
            ))
        }
    };
    let bytes = bs58::decode(encoded)
        .into_vec()
        .map_err(|_| AuthError::Malformed("public key is not base58".into()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AuthError::Malformed("ed25519 public key must be 32 bytes".into()))?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| AuthError::Malformed("public key is not a valid ed25519 point".into()))
}

/// Render a verifying key as `ed25519:<base58>`.
pub fn format_public_key(key: &VerifyingKey) -> String {
    format!("ed25519:{}", bs58::encode(key.as_bytes()).into_string())
}

/// The authentication fields a client sends (as headers in the API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRequest {
    /// `X-NEAR-Account`
    pub account_id: String,
    /// `X-NEAR-Public-Key`, `ed25519:<base58>`
    pub public_key: String,
    /// `X-NEAR-Signature`, standard base64 of 64 bytes
    pub signature_b64: String,
    /// `X-NEAR-Nonce`, standard base64 of 32 bytes
    pub nonce_b64: String,
    /// `X-NEAR-Timestamp`, unix milliseconds
    pub timestamp_ms: u64,
}

impl SignedRequest {
    /// Sign a request as a client would. Used by tests and by agent
    /// clients that hold a full-access identity key.
    #[allow(clippy::too_many_arguments)]
    pub fn sign(
        signing_key: &SigningKey,
        account_id: &str,
        recipient: &str,
        method: &str,
        path: &str,
        body: &[u8],
        nonce: [u8; 32],
        timestamp_ms: u64,
    ) -> Result<Self, AuthError> {
        let message = canonical_message(method, path, body, timestamp_ms)?;
        let hash = payload_hash(&message, &nonce, recipient, None);
        let signature = signing_key.sign(&hash);
        Ok(SignedRequest {
            account_id: account_id.to_string(),
            public_key: format_public_key(&signing_key.verifying_key()),
            signature_b64: BASE64.encode(signature.to_bytes()),
            nonce_b64: BASE64.encode(nonce),
            timestamp_ms,
        })
    }
}

/// Permission of an access key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPermission {
    FullAccess,
    FunctionCall,
}

/// Result of looking up one key on one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyLookup {
    Found(KeyPermission),
    /// The account exists but does not hold the key (or it was deleted).
    Missing,
    /// The account does not exist (nearcore 2.14 `UNKNOWN_ACCOUNT`).
    UnknownAccount,
}

/// Look up one key on one account. The production implementation is a
/// read-only RPC `query` with `request_type: "view_access_key"` at
/// `finality: "final"`; see `docs/operations/auth.md`.
pub trait AccessKeyLookup {
    fn lookup(&self, account_id: &str, public_key: &str) -> Result<KeyLookup, AuthError>;
}

impl<T: AccessKeyLookup + ?Sized> AccessKeyLookup for &T {
    fn lookup(&self, account_id: &str, public_key: &str) -> Result<KeyLookup, AuthError> {
        (**self).lookup(account_id, public_key)
    }
}

/// One page of `view_access_key_list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyListPage {
    pub keys: Vec<(String, KeyPermission)>,
    /// Cursor for the next page (`after_key`); `None` on the last page.
    pub next_after: Option<String>,
}

/// Result of fetching one page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListResult {
    Page(KeyListPage),
    UnknownAccount,
}

/// A paginated `view_access_key_list` source (nearcore 2.14 adds
/// `after_key` and `limit`, and caps an unpaginated list at 100 keys).
pub trait AccessKeyListSource {
    fn list_page(
        &self,
        account_id: &str,
        after_key: Option<&str>,
        limit: u32,
    ) -> Result<ListResult, AuthError>;
}

/// An [`AccessKeyLookup`] over a paginated key list, for RPC nodes where
/// the single-key view is unavailable.
pub struct PaginatedLookup<S> {
    source: S,
    page_limit: u32,
    max_pages: u32,
}

impl<S> PaginatedLookup<S> {
    /// `page_limit` keys per page, at most `max_pages` pages per lookup.
    pub fn new(source: S, page_limit: u32, max_pages: u32) -> Self {
        PaginatedLookup {
            source,
            page_limit: page_limit.max(1),
            max_pages: max_pages.max(1),
        }
    }
}

impl<S: AccessKeyListSource> AccessKeyLookup for PaginatedLookup<S> {
    fn lookup(&self, account_id: &str, public_key: &str) -> Result<KeyLookup, AuthError> {
        let mut after: Option<String> = None;
        for _ in 0..self.max_pages {
            let page = match self
                .source
                .list_page(account_id, after.as_deref(), self.page_limit)?
            {
                ListResult::UnknownAccount => return Ok(KeyLookup::UnknownAccount),
                ListResult::Page(page) => page,
            };
            if let Some((_, permission)) = page.keys.iter().find(|(key, _)| key == public_key) {
                return Ok(KeyLookup::Found(permission.clone()));
            }
            match page.next_after {
                None => return Ok(KeyLookup::Missing),
                Some(next) if Some(&next) == after.as_ref() => {
                    return Err(AuthError::LookupUnavailable(
                        "key list cursor did not advance".into(),
                    ))
                }
                Some(next) => after = Some(next),
            }
        }
        Err(AuthError::TooManyKeyPages)
    }
}

/// Caches positive full-access results for a short TTL. Misses and
/// function-call results are never cached, so a newly added key works at
/// once, and a deleted key stops working within the TTL.
pub struct CachedLookup<L> {
    inner: L,
    ttl: Duration,
    cache: Mutex<HashMap<(String, String), Instant>>,
}

impl<L> CachedLookup<L> {
    pub fn new(inner: L, ttl: Duration) -> Self {
        CachedLookup {
            inner,
            ttl,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

impl<L: AccessKeyLookup> AccessKeyLookup for CachedLookup<L> {
    fn lookup(&self, account_id: &str, public_key: &str) -> Result<KeyLookup, AuthError> {
        let key = (account_id.to_string(), public_key.to_string());
        if let Ok(cache) = self.cache.lock() {
            if let Some(stored) = cache.get(&key) {
                if stored.elapsed() < self.ttl {
                    return Ok(KeyLookup::Found(KeyPermission::FullAccess));
                }
            }
        }
        let result = self.inner.lookup(account_id, public_key)?;
        if let Ok(mut cache) = self.cache.lock() {
            if result == KeyLookup::Found(KeyPermission::FullAccess) {
                cache.insert(key, Instant::now());
            } else {
                cache.remove(&key);
            }
        }
        Ok(result)
    }
}

/// In-memory key registry for tests and local demos.
#[derive(Default)]
pub struct MemoryKeyLookup {
    accounts: Mutex<HashMap<String, Vec<(String, KeyPermission)>>>,
}

impl MemoryKeyLookup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a key to an account, creating the account.
    pub fn add_key(&self, account_id: &str, public_key: &str, permission: KeyPermission) {
        if let Ok(mut accounts) = self.accounts.lock() {
            accounts
                .entry(account_id.to_string())
                .or_default()
                .push((public_key.to_string(), permission));
        }
    }

    /// Delete a key from an account.
    pub fn delete_key(&self, account_id: &str, public_key: &str) {
        if let Ok(mut accounts) = self.accounts.lock() {
            if let Some(keys) = accounts.get_mut(account_id) {
                keys.retain(|(key, _)| key != public_key);
            }
        }
    }
}

impl AccessKeyLookup for MemoryKeyLookup {
    fn lookup(&self, account_id: &str, public_key: &str) -> Result<KeyLookup, AuthError> {
        let accounts = self
            .accounts
            .lock()
            .map_err(|_| AuthError::LookupUnavailable("lock poisoned".into()))?;
        Ok(match accounts.get(account_id) {
            None => KeyLookup::UnknownAccount,
            Some(keys) => keys
                .iter()
                .find(|(key, _)| key == public_key)
                .map(|(_, permission)| KeyLookup::Found(permission.clone()))
                .unwrap_or(KeyLookup::Missing),
        })
    }
}

/// Single-use nonce storage.
pub trait NonceStore {
    /// Record a nonce for an account until `expires_at_ms`. Returns true
    /// when it was fresh, false on replay. Must be atomic.
    fn consume(
        &self,
        account_id: &str,
        nonce: &[u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<bool, AuthError>;
}

impl<T: NonceStore + ?Sized> NonceStore for &T {
    fn consume(
        &self,
        account_id: &str,
        nonce: &[u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<bool, AuthError> {
        (**self).consume(account_id, nonce, expires_at_ms, now_ms)
    }
}

/// Process-local nonce store. Loses its contents on restart; use the
/// SQLite store when replay protection must survive a restart inside the
/// timestamp window.
#[derive(Default)]
pub struct MemoryNonceStore {
    seen: Mutex<HashMap<(String, [u8; 32]), u64>>,
}

impl MemoryNonceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl NonceStore for MemoryNonceStore {
    fn consume(
        &self,
        account_id: &str,
        nonce: &[u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<bool, AuthError> {
        let mut seen = self
            .seen
            .lock()
            .map_err(|_| AuthError::Store("nonce store lock poisoned".into()))?;
        seen.retain(|_, expiry| *expiry >= now_ms);
        let key = (account_id.to_string(), *nonce);
        if seen.contains_key(&key) {
            return Ok(false);
        }
        seen.insert(key, expires_at_ms);
        Ok(true)
    }
}

impl NonceStore for aps_store::SqliteStore {
    fn consume(
        &self,
        account_id: &str,
        nonce: &[u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<bool, AuthError> {
        Ok(self.consume_nonce(account_id, nonce, expires_at_ms, now_ms)?)
    }
}

/// Verifier settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierConfig {
    /// The fixed NEP-413 `recipient` of this service.
    pub recipient: String,
    /// Oldest accepted timestamp, relative to now.
    pub max_age_ms: u64,
    /// Furthest-future accepted timestamp, relative to now.
    pub max_future_skew_ms: u64,
}

impl VerifierConfig {
    /// Five minutes back, thirty seconds forward.
    pub fn new(recipient: impl Into<String>) -> Self {
        VerifierConfig {
            recipient: recipient.into(),
            max_age_ms: 300_000,
            max_future_skew_ms: 30_000,
        }
    }
}

/// A successfully authenticated NEAR account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedAccount {
    pub account_id: String,
    pub public_key: String,
}

/// Verifies NEP-413 signed requests.
pub struct Nep413Verifier<L, N> {
    config: VerifierConfig,
    lookup: L,
    nonces: N,
}

impl<L: AccessKeyLookup, N: NonceStore> Nep413Verifier<L, N> {
    pub fn new(config: VerifierConfig, lookup: L, nonces: N) -> Self {
        Nep413Verifier {
            config,
            lookup,
            nonces,
        }
    }

    /// The configuration in force.
    pub fn config(&self) -> &VerifierConfig {
        &self.config
    }

    /// Verify one request. Order: parse, timestamp window, signature over
    /// the rebuilt payload, key permission, then nonce consumption, so a
    /// forged request can never burn a legitimate nonce.
    pub fn verify(
        &self,
        request: &SignedRequest,
        method: &str,
        path: &str,
        body: &[u8],
        now_ms: u64,
    ) -> Result<VerifiedAccount, AuthError> {
        if !is_valid_account_id(&request.account_id) {
            return Err(AuthError::Malformed(
                "account id is not a NEAR account id".into(),
            ));
        }
        let verifying_key = parse_public_key(&request.public_key)?;
        let signature_bytes: [u8; 64] = BASE64
            .decode(&request.signature_b64)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| AuthError::Malformed("signature must be base64 of 64 bytes".into()))?;
        let nonce: [u8; 32] = BASE64
            .decode(&request.nonce_b64)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| AuthError::Malformed("nonce must be base64 of 32 bytes".into()))?;

        if request.timestamp_ms.saturating_add(self.config.max_age_ms) < now_ms {
            return Err(AuthError::TimestampExpired);
        }
        if request.timestamp_ms > now_ms.saturating_add(self.config.max_future_skew_ms) {
            return Err(AuthError::TimestampInFuture);
        }

        let message = canonical_message(method, path, body, request.timestamp_ms)?;
        let hash = payload_hash(&message, &nonce, &self.config.recipient, None);
        let signature = Signature::from_bytes(&signature_bytes);
        verifying_key
            .verify_strict(&hash, &signature)
            .map_err(|_| AuthError::BadSignature)?;

        match self
            .lookup
            .lookup(&request.account_id, &request.public_key)?
        {
            KeyLookup::Found(KeyPermission::FullAccess) => {}
            KeyLookup::Found(KeyPermission::FunctionCall) => {
                return Err(AuthError::KeyNotFullAccess)
            }
            KeyLookup::Missing => return Err(AuthError::KeyNotFound),
            KeyLookup::UnknownAccount => return Err(AuthError::UnknownAccount),
        }

        let expires_at = request
            .timestamp_ms
            .saturating_add(self.config.max_age_ms)
            .saturating_add(1);
        if !self
            .nonces
            .consume(&request.account_id, &nonce, expires_at, now_ms)?
        {
            return Err(AuthError::NonceReplayed);
        }
        Ok(VerifiedAccount {
            account_id: request.account_id.clone(),
            public_key: request.public_key.clone(),
        })
    }
}

/// Parse one page of a `view_access_key_list` RPC `result` object:
/// `{keys: [{public_key, access_key: {nonce, permission}}], ...}`, with
/// `permission` either `"FullAccess"` or `{"FunctionCall": {...}}`. The
/// next cursor is the last key when the page is full.
pub fn parse_access_key_list(
    result: &serde_json::Value,
    page_limit: u32,
) -> Result<KeyListPage, AuthError> {
    let bad = || AuthError::LookupUnavailable("unexpected view_access_key_list shape".into());
    let keys = result
        .get("keys")
        .and_then(|v| v.as_array())
        .ok_or_else(bad)?;
    let mut parsed = Vec::with_capacity(keys.len());
    for entry in keys {
        let public_key = entry
            .get("public_key")
            .and_then(|v| v.as_str())
            .ok_or_else(bad)?;
        let permission = entry
            .get("access_key")
            .and_then(|v| v.get("permission"))
            .ok_or_else(bad)?;
        parsed.push((
            public_key.to_string(),
            parse_permission(permission).ok_or_else(bad)?,
        ));
    }
    let next_after = if parsed.len() as u32 >= page_limit {
        parsed.last().map(|(key, _)| key.clone())
    } else {
        None
    };
    Ok(KeyListPage {
        keys: parsed,
        next_after,
    })
}

/// Parse a `permission` value.
pub fn parse_permission(value: &serde_json::Value) -> Option<KeyPermission> {
    if value.as_str() == Some("FullAccess") {
        return Some(KeyPermission::FullAccess);
    }
    if value.get("FunctionCall").is_some() {
        return Some(KeyPermission::FunctionCall);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_bytes_match_the_nep() {
        assert_eq!(NEP413_TAG, (1u32 << 31) + 413);
        assert_eq!(NEP413_TAG.to_le_bytes(), [0x9d, 0x01, 0x00, 0x80]);
    }

    #[test]
    fn nep_example_payload_layout() {
        // NEP-413 example inputs: message "hi", nonce [0..31], recipient
        // "myapp.com", callbackUrl "myapp.com/callback".
        let mut nonce = [0u8; 32];
        for (i, b) in nonce.iter_mut().enumerate() {
            *b = i as u8;
        }
        let bytes = payload_bytes("hi", &nonce, "myapp.com", Some("myapp.com/callback"));
        let mut expected = vec![0x9d, 0x01, 0x00, 0x80, 2, 0, 0, 0, b'h', b'i'];
        expected.extend_from_slice(&nonce);
        expected.extend_from_slice(&[9, 0, 0, 0]);
        expected.extend_from_slice(b"myapp.com");
        expected.push(1);
        expected.extend_from_slice(&[18, 0, 0, 0]);
        expected.extend_from_slice(b"myapp.com/callback");
        assert_eq!(bytes, expected);
        let none = payload_bytes("hi", &nonce, "myapp.com", None);
        assert_eq!(*none.last().unwrap(), 0);
        assert_eq!(none.len(), 4 + 6 + 32 + 13 + 1);
    }

    #[test]
    fn canonical_message_is_fixed_json() {
        let message = canonical_message("POST", "/reservations?x=1", b"{}", 42).unwrap();
        assert_eq!(
            message,
            "{\"scheme\":\"aps-nep413-request-v1\",\"method\":\"POST\",\"path\":\"/reservations?x=1\",\
             \"body_sha256\":\"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a\",\
             \"timestamp_ms\":42}"
        );
        assert!(canonical_message("post", "/x", b"", 1).is_err());
        assert!(canonical_message("GET", "x", b"", 1).is_err());
        assert!(canonical_message("GET", "/a b", b"", 1).is_err());
        assert!(canonical_message("GET", "/a\"b", b"", 1).is_err());
    }

    #[test]
    fn account_ids() {
        for good in [
            "alice.testnet",
            "a1",
            "sub_acc-1.alice.near",
            &"0".repeat(64),
        ] {
            assert!(is_valid_account_id(good), "{good}");
        }
        for bad in [
            "a",
            "Alice.near",
            "a..b",
            ".a",
            "a-",
            "a--b",
            "a b",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_account_id(bad), "{bad}");
        }
    }

    #[test]
    fn public_key_parsing() {
        let key = SigningKey::from_bytes(&[5; 32]).verifying_key();
        let text = format_public_key(&key);
        assert_eq!(parse_public_key(&text).unwrap(), key);
        assert_eq!(
            parse_public_key("secp256k1:abc").unwrap_err(),
            AuthError::UnsupportedKeyType
        );
        assert!(parse_public_key("ed25519:0OIl").is_err());
        assert!(parse_public_key("abc").is_err());
    }

    #[test]
    fn key_list_page_parsing() {
        let result = serde_json::json!({
            "block_hash": "x", "block_height": 1,
            "keys": [
                {"public_key": "ed25519:A", "access_key": {"nonce": 1, "permission": "FullAccess"}},
                {"public_key": "ed25519:B", "access_key": {"nonce": 2, "permission":
                    {"FunctionCall": {"allowance": null, "receiver_id": "c.near", "method_names": []}}}}
            ]
        });
        let page = parse_access_key_list(&result, 2).unwrap();
        assert_eq!(page.keys[0].1, KeyPermission::FullAccess);
        assert_eq!(page.keys[1].1, KeyPermission::FunctionCall);
        assert_eq!(page.next_after.as_deref(), Some("ed25519:B"));
        assert_eq!(
            parse_access_key_list(&result, 100).unwrap().next_after,
            None
        );
        assert!(parse_access_key_list(&serde_json::json!({}), 2).is_err());
    }
}
