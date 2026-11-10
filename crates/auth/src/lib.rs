//! TriagePay caller authentication primitives.
//!
//! Two methods, both resolving to a caller identity that the ledger's
//! caller-scoped idempotency already understands:
//!
//! 1. **API keys** ([`apikey`]): 32 random bytes from the OS CSPRNG,
//!    shown once as `aps_<64 hex>`, stored only as SHA-256 of the bytes.
//! 2. **NEP-413 signed requests** ([`nep413`]): the caller signs a
//!    canonical description of the HTTP request with an ed25519
//!    **full-access** key of their NEAR account; the server verifies the
//!    signature, the key's permission through an injected lookup, a
//!    timestamp window, and single use of the nonce.
//!
//! No HTTP wiring lives here. The API crate maps these results to
//! requests and responses.

pub mod apikey;
pub mod error;
pub mod nep413;

pub use apikey::{authenticate_api_key, create_caller, parse_bearer, rotate_caller, ApiKey};
pub use error::AuthError;
pub use nep413::{
    AccessKeyListSource, AccessKeyLookup, CachedLookup, KeyListPage, KeyLookup, KeyPermission,
    ListResult, MemoryKeyLookup, MemoryNonceStore, Nep413Verifier, NonceStore, PaginatedLookup,
    SignedRequest, VerifiedAccount, VerifierConfig,
};
