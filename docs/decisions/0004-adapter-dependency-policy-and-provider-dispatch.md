# ADR 0004: Adapter dependency policy and provider dispatch

Status: accepted

## Context

aps-core and aps-payment are dependency-free by policy (ADR-0001); the
HTTP API (aps-api) already depends on axum/tokio/serde. A real payment
adapter must speak JSON-RPC over HTTPS, which requires an HTTP client and
base64 handling. The existing `PaymentProvider` trait is synchronous
(`&mut self`), while the API layer is async.

## Decision

1. Adapter crates live in their own workspace member (`aps-near`) and may
   carry the minimum dependencies their protocol needs: `ureq` (blocking
   HTTP over rustls), `serde_json`, `base64`. aps-core and aps-payment
   stay dependency-free; no adapter code is imported by them.
2. The API crate dispatches through a `Provider` enum
   (`Mock(MockProvider) | Near(NearProvider)`) rather than a trait
   object. The mock stays the default; the near provider activates only
   via explicit environment configuration.
3. Blocking HTTP runs on tokio's blocking pool: the near provider exposes
   a two-step observation (`prepare_fetch` → `verify_tx` →
   `apply_verification`) so the enum's async `observe` can move the
   chain fetch into `spawn_blocking` without holding a lock across it.
4. The provider clock is synced to wall time before every provider
   operation (the mock's pattern), keeping expiry deterministic and
   testable without sleeps.

## Consequences

- The async boundary is explicit and testable offline: the verification
  logic is a pure function over the RPC client, exercised against a
  loopback stub server in tests.
- Adding a second real provider (NEAR Intents 1Click) means a new enum
  variant, not a trait-object redesign.
- ureq's `send_string` defaults to `Content-Type: text/plain`; the NEAR
  RPC requires `application/json` (415 otherwise) - the client sets it
  explicitly, and the live read-only tests pin this behavior.
