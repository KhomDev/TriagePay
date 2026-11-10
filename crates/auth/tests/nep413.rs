//! NEP-413 verifier tests with ed25519 keys generated in-test from fixed
//! seeds. No network access: key ownership comes from an in-memory lookup.

use std::cell::Cell;
use std::time::Duration;

use aps_auth::nep413::format_public_key;
use aps_auth::{
    AccessKeyListSource, AccessKeyLookup, AuthError, CachedLookup, KeyListPage, KeyLookup,
    KeyPermission, ListResult, MemoryKeyLookup, MemoryNonceStore, Nep413Verifier, PaginatedLookup,
    SignedRequest, VerifierConfig,
};
use ed25519_dalek::SigningKey;

const RECIPIENT: &str = "triagepay.test";
const NOW: u64 = 1_762_000_000_000;
const BODY: &[u8] = br#"{"account_id":"alice.testnet","sku":"scan.v1"}"#;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn pk(seed: u8) -> String {
    format_public_key(&key(seed).verifying_key())
}

fn nonce(n: u8) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[0] = n;
    out[31] = 0xa5;
    out
}

fn signed(seed: u8, account: &str, n: u8, ts: u64) -> SignedRequest {
    SignedRequest::sign(
        &key(seed),
        account,
        RECIPIENT,
        "POST",
        "/reservations",
        BODY,
        nonce(n),
        ts,
    )
    .unwrap()
}

fn lookup() -> MemoryKeyLookup {
    let keys = MemoryKeyLookup::new();
    keys.add_key("alice.testnet", &pk(1), KeyPermission::FullAccess);
    keys.add_key("alice.testnet", &pk(2), KeyPermission::FunctionCall);
    keys.add_key("bob.testnet", &pk(3), KeyPermission::FullAccess);
    keys
}

fn verify(
    verifier: &Nep413Verifier<&MemoryKeyLookup, &MemoryNonceStore>,
    req: &SignedRequest,
) -> Result<String, AuthError> {
    verifier
        .verify(req, "POST", "/reservations", BODY, NOW)
        .map(|v| v.account_id)
}

#[test]
fn a_valid_request_authenticates() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    assert_eq!(
        verify(&verifier, &signed(1, "alice.testnet", 1, NOW)).unwrap(),
        "alice.testnet"
    );
    // Inside the window on both sides.
    assert!(verify(&verifier, &signed(1, "alice.testnet", 2, NOW - 299_000)).is_ok());
    assert!(verify(&verifier, &signed(1, "alice.testnet", 3, NOW + 29_000)).is_ok());
}

#[test]
fn a_signature_by_the_wrong_key_fails() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    // Signed with key 9, but claims alice's registered key 1.
    let mut forged = signed(9, "alice.testnet", 1, NOW);
    forged.public_key = pk(1);
    assert_eq!(
        verify(&verifier, &forged).unwrap_err(),
        AuthError::BadSignature
    );
    // Signed with key 9 and honest about it: key 9 is not on the account.
    assert_eq!(
        verify(&verifier, &signed(9, "alice.testnet", 2, NOW)).unwrap_err(),
        AuthError::KeyNotFound
    );
}

#[test]
fn a_replayed_nonce_fails() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    let request = signed(1, "alice.testnet", 7, NOW);
    assert!(verify(&verifier, &request).is_ok());
    assert_eq!(
        verify(&verifier, &request).unwrap_err(),
        AuthError::NonceReplayed
    );
}

#[test]
fn a_forged_request_does_not_burn_a_nonce() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    let mut forged = signed(9, "alice.testnet", 8, NOW);
    forged.public_key = pk(1);
    assert!(verify(&verifier, &forged).is_err());
    assert!(verify(&verifier, &signed(1, "alice.testnet", 8, NOW)).is_ok());
}

#[test]
fn expired_and_future_timestamps_fail() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    assert_eq!(
        verify(&verifier, &signed(1, "alice.testnet", 1, NOW - 300_001)).unwrap_err(),
        AuthError::TimestampExpired
    );
    assert_eq!(
        verify(&verifier, &signed(1, "alice.testnet", 2, NOW + 30_001)).unwrap_err(),
        AuthError::TimestampInFuture
    );
    // A signed timestamp cannot be moved into the window.
    let mut moved = signed(1, "alice.testnet", 3, NOW - 400_000);
    moved.timestamp_ms = NOW;
    assert_eq!(
        verify(&verifier, &moved).unwrap_err(),
        AuthError::BadSignature
    );
}

#[test]
fn a_key_of_a_different_account_fails() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    // Bob's full-access key, claiming to be alice.
    assert_eq!(
        verify(&verifier, &signed(3, "alice.testnet", 1, NOW)).unwrap_err(),
        AuthError::KeyNotFound
    );
    assert_eq!(
        verify(&verifier, &signed(3, "nobody.testnet", 1, NOW)).unwrap_err(),
        AuthError::UnknownAccount
    );
}

#[test]
fn a_function_call_key_is_not_enough() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    let err = verify(&verifier, &signed(2, "alice.testnet", 1, NOW)).unwrap_err();
    assert_eq!(err, AuthError::KeyNotFullAccess);
    assert_eq!(err.http_status(), 403);
}

#[test]
fn a_deleted_key_fails() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    assert!(verify(&verifier, &signed(1, "alice.testnet", 1, NOW)).is_ok());
    keys.delete_key("alice.testnet", &pk(1));
    assert_eq!(
        verify(&verifier, &signed(1, "alice.testnet", 2, NOW)).unwrap_err(),
        AuthError::KeyNotFound
    );
}

#[test]
fn binding_covers_recipient_method_path_and_body() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    let other_app = SignedRequest::sign(
        &key(1),
        "alice.testnet",
        "other.app",
        "POST",
        "/reservations",
        BODY,
        nonce(1),
        NOW,
    )
    .unwrap();
    assert_eq!(
        verify(&verifier, &other_app).unwrap_err(),
        AuthError::BadSignature
    );
    let request = signed(1, "alice.testnet", 2, NOW);
    for (method, path, body) in [
        ("PUT", "/reservations", BODY),
        ("POST", "/deposits", BODY),
        ("POST", "/reservations", &b"{}"[..]),
    ] {
        assert_eq!(
            verifier
                .verify(&request, method, path, body, NOW)
                .unwrap_err(),
            AuthError::BadSignature
        );
    }
}

#[test]
fn malformed_fields_are_rejected_before_lookup() {
    let (keys, nonces) = (lookup(), MemoryNonceStore::new());
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &nonces);
    let good = signed(1, "alice.testnet", 1, NOW);
    let mut cases = Vec::new();
    let mut r = good.clone();
    r.signature_b64 = "AAAA".into();
    cases.push(r);
    let mut r = good.clone();
    r.nonce_b64 = "AAAA".into();
    cases.push(r);
    let mut r = good.clone();
    r.account_id = "Alice".into();
    cases.push(r);
    for case in cases {
        assert!(matches!(
            verify(&verifier, &case),
            Err(AuthError::Malformed(_))
        ));
    }
    let mut r = good;
    r.public_key = "secp256k1:abc".into();
    assert_eq!(
        verify(&verifier, &r).unwrap_err(),
        AuthError::UnsupportedKeyType
    );
}

#[test]
fn sqlite_nonces_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("aps-auth-nonce-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("auth.sqlite");
    let keys = lookup();
    let request = signed(1, "alice.testnet", 4, NOW);
    {
        let store = aps_store::SqliteStore::open(&path).unwrap();
        let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &store);
        assert!(verifier
            .verify(&request, "POST", "/reservations", BODY, NOW)
            .is_ok());
    }
    let store = aps_store::SqliteStore::open(&path).unwrap();
    let verifier = Nep413Verifier::new(VerifierConfig::new(RECIPIENT), &keys, &store);
    assert_eq!(
        verifier
            .verify(&request, "POST", "/reservations", BODY, NOW + 1_000)
            .unwrap_err(),
        AuthError::NonceReplayed
    );
    drop(verifier);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A key list served in pages of `per_page`, counting fetches.
struct PagedKeys {
    keys: Vec<(String, KeyPermission)>,
    per_page: usize,
    fetches: Cell<u32>,
}

impl AccessKeyListSource for PagedKeys {
    fn list_page(
        &self,
        account_id: &str,
        after_key: Option<&str>,
        _limit: u32,
    ) -> Result<ListResult, AuthError> {
        self.fetches.set(self.fetches.get() + 1);
        if account_id != "many.testnet" {
            return Ok(ListResult::UnknownAccount);
        }
        let start = match after_key {
            None => 0,
            Some(after) => self.keys.iter().position(|(k, _)| k == after).unwrap() + 1,
        };
        let end = (start + self.per_page).min(self.keys.len());
        let page = self.keys[start..end].to_vec();
        let next_after = if end < self.keys.len() {
            page.last().map(|(k, _)| k.clone())
        } else {
            None
        };
        Ok(ListResult::Page(KeyListPage {
            keys: page,
            next_after,
        }))
    }
}

fn paged(per_page: usize) -> PagedKeys {
    let mut keys: Vec<(String, KeyPermission)> = (10..=250u8)
        .map(|seed| (pk(seed), KeyPermission::FunctionCall))
        .collect();
    keys.push((pk(1), KeyPermission::FullAccess));
    PagedKeys {
        keys,
        per_page,
        fetches: Cell::new(0),
    }
}

#[test]
fn pagination_finds_a_key_past_the_first_page() {
    let source = paged(100);
    let lookup = PaginatedLookup::new(&source, 100, 10);
    assert_eq!(
        lookup.lookup("many.testnet", &pk(1)).unwrap(),
        KeyLookup::Found(KeyPermission::FullAccess)
    );
    assert_eq!(source.fetches.get(), 3, "242 keys at 100 per page");
    assert_eq!(
        lookup.lookup("many.testnet", &pk(9)).unwrap(),
        KeyLookup::Missing
    );
    assert_eq!(
        lookup.lookup("ghost.testnet", &pk(1)).unwrap(),
        KeyLookup::UnknownAccount
    );
    let capped = PaginatedLookup::new(&source, 100, 2);
    assert_eq!(
        capped.lookup("many.testnet", &pk(1)).unwrap_err(),
        AuthError::TooManyKeyPages
    );
}

impl AccessKeyListSource for &PagedKeys {
    fn list_page(
        &self,
        account_id: &str,
        after_key: Option<&str>,
        limit: u32,
    ) -> Result<ListResult, AuthError> {
        (**self).list_page(account_id, after_key, limit)
    }
}

#[test]
fn the_cache_keeps_only_full_access_hits_for_its_ttl() {
    let keys = lookup();
    let cached = CachedLookup::new(&keys, Duration::from_secs(60));
    assert_eq!(
        cached.lookup("alice.testnet", &pk(1)).unwrap(),
        KeyLookup::Found(KeyPermission::FullAccess)
    );
    keys.delete_key("alice.testnet", &pk(1));
    // Within the TTL the cached hit still answers: the documented bound.
    assert_eq!(
        cached.lookup("alice.testnet", &pk(1)).unwrap(),
        KeyLookup::Found(KeyPermission::FullAccess)
    );
    let uncached = CachedLookup::new(&keys, Duration::ZERO);
    assert_eq!(
        uncached.lookup("alice.testnet", &pk(1)).unwrap(),
        KeyLookup::Missing
    );
    // Function-call results are never cached as full access.
    assert_eq!(
        cached.lookup("alice.testnet", &pk(2)).unwrap(),
        KeyLookup::Found(KeyPermission::FunctionCall)
    );
}
