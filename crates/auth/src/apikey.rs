//! API keys: 32 random bytes, shown once, stored as SHA-256 only.

use std::fmt;

use aps_store::{to_hex, CallerTable};
use zeroize::Zeroize;

use crate::error::AuthError;

/// Prefix of the presented token, so leaked keys are easy to grep for.
pub const API_KEY_PREFIX: &str = "aps_";

/// A raw API key. Zeroed on drop; `Debug` never prints it.
pub struct ApiKey([u8; 32]);

impl ApiKey {
    /// A fresh key from the OS CSPRNG.
    pub fn generate() -> Result<Self, AuthError> {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|err| AuthError::Random(err.to_string()))?;
        Ok(ApiKey(bytes))
    }

    /// A key from known bytes (tests).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        ApiKey(bytes)
    }

    /// Parse a presented token: `aps_` followed by 64 lowercase hex.
    pub fn parse(token: &str) -> Result<Self, AuthError> {
        let hex = token
            .strip_prefix(API_KEY_PREFIX)
            .ok_or(AuthError::ApiKeyMalformed)?;
        if hex.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(AuthError::ApiKeyMalformed);
        }
        aps_store::from_hex32(hex)
            .map(ApiKey)
            .ok_or(AuthError::ApiKeyMalformed)
    }

    /// The token to hand to the caller, once.
    pub fn token(&self) -> String {
        format!("{API_KEY_PREFIX}{}", to_hex(&self.0))
    }

    /// SHA-256 of the 32 key bytes: the only form that is stored.
    pub fn digest(&self) -> [u8; 32] {
        aps_store::sha256(&self.0)
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Extract the token from an `Authorization: Bearer <token>` value.
pub fn parse_bearer(header: &str) -> Option<&str> {
    let (scheme, token) = header.trim().split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
        Some(token.trim())
    } else {
        None
    }
}

/// Resolve a presented token to its active caller id.
///
/// Lookup is by digest, so the comparison never touches key material in
/// a timing-sensitive way. Unknown and revoked keys fail identically.
pub fn authenticate_api_key<T: CallerTable + ?Sized>(
    table: &T,
    token: &str,
) -> Result<String, AuthError> {
    let key = ApiKey::parse(token)?;
    match table.find_by_key_hash(&key.digest())? {
        Some(record) if record.is_active() => Ok(record.caller_id),
        _ => Err(AuthError::ApiKeyRejected),
    }
}

/// Generate a key for a new caller and register its digest. The returned
/// key is the only copy; the caller of this function must hand it over
/// and drop it.
pub fn create_caller<T: CallerTable + ?Sized>(
    table: &mut T,
    caller_id: &str,
    now: u64,
) -> Result<ApiKey, AuthError> {
    let key = ApiKey::generate()?;
    table.insert_caller(caller_id, key.digest(), now)?;
    Ok(key)
}

/// Replace an active caller's key. The old key stops working at once.
pub fn rotate_caller<T: CallerTable + ?Sized>(
    table: &mut T,
    caller_id: &str,
) -> Result<ApiKey, AuthError> {
    let key = ApiKey::generate()?;
    if table.replace_key(caller_id, key.digest())? {
        Ok(key)
    } else {
        Err(AuthError::Store(format!(
            "caller {caller_id} is unknown or revoked"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aps_store::MemoryCallers;

    #[test]
    fn tokens_round_trip_and_digest_is_sha256_of_bytes() {
        let key = ApiKey::from_bytes([0xab; 32]);
        let token = key.token();
        assert_eq!(token.len(), 4 + 64);
        let parsed = ApiKey::parse(&token).unwrap();
        assert_eq!(parsed.digest(), aps_store::sha256(&[0xab; 32]));
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let good = ApiKey::from_bytes([1; 32]).token();
        for bad in [
            String::new(),
            good[4..].to_string(),
            format!("apk_{}", &good[4..]),
            good.to_uppercase(),
            format!("{good}0"),
            good[..67].to_string(),
        ] {
            assert_eq!(ApiKey::parse(&bad).unwrap_err(), AuthError::ApiKeyMalformed);
        }
    }

    #[test]
    fn generated_keys_differ() {
        let a = ApiKey::generate().unwrap();
        let b = ApiKey::generate().unwrap();
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn bearer_parsing() {
        assert_eq!(parse_bearer("Bearer abc"), Some("abc"));
        assert_eq!(parse_bearer("bearer  abc "), Some("abc"));
        assert_eq!(parse_bearer("Basic abc"), None);
        assert_eq!(parse_bearer("Bearer"), None);
    }

    #[test]
    fn create_authenticate_rotate_revoke() {
        let mut table = MemoryCallers::new();
        let key = create_caller(&mut table, "agent-1", 1).unwrap();
        let token = key.token();
        assert_eq!(authenticate_api_key(&table, &token).unwrap(), "agent-1");
        let stored = table.list_callers().unwrap();
        assert_eq!(stored[0].key_sha256, key.digest());

        let rotated = rotate_caller(&mut table, "agent-1").unwrap();
        assert_eq!(
            authenticate_api_key(&table, &token).unwrap_err(),
            AuthError::ApiKeyRejected
        );
        let new_token = rotated.token();
        assert_eq!(authenticate_api_key(&table, &new_token).unwrap(), "agent-1");

        table.revoke_caller("agent-1", 2).unwrap();
        assert_eq!(
            authenticate_api_key(&table, &new_token).unwrap_err(),
            AuthError::ApiKeyRejected
        );
        let unknown = ApiKey::from_bytes([3; 32]).token();
        assert_eq!(
            authenticate_api_key(&table, &unknown).unwrap_err(),
            AuthError::ApiKeyRejected
        );
        assert!(rotate_caller(&mut table, "agent-1").is_err());
    }
}
