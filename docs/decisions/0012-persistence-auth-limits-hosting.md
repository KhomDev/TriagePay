# ADR 0012: Persistence, authentication, limits, and hosting

Status: accepted. Wave 1 (2026-11-04 to 11-06) implements the store, the authentication
primitives, the container and provider configuration, and the UI. Wiring them into `aps-api` is
wave 2 and is marked **planned** below.

## Context

The gateway was a loopback-only, in-memory, single-caller demonstrator: state was lost on
restart, every request acted as the hard-coded `dev-agent`, nothing limited abuse, and judges
had no way to try it. `PROPOSALS.md` names the durable ledger as the top credibility gap and the
single caller as the largest real security limitation.

Constraints carried from earlier decisions:

- `aps-core` stays dependency-free and is the only authority on money movement (ADR-0001).
- Idempotency is scoped to the caller (ADR-0003), so authentication must produce a caller id.
- Model output never moves money (standing rule S9).

## Decision

### 1. Persistence: `crates/store` (`aps-store`)

- **Append-only event journal in SQLite** (`rusqlite =0.37.0`, `bundled`). One row per
  successful mutation: deposit, reserve, start, settle, refund, payment-intent state change, and
  API-level idempotency record. Replays and failed operations write nothing. SQL triggers abort
  every `UPDATE` and `DELETE` on the `events` table.
- **Hash chain.** `hash(n) = SHA-256("aps-journal-v1" || prev_hash || seq || len(kind) || kind
  || len(body) || body)`, with an all-zero genesis. An edited, deleted or reordered row fails the
  open with `ChainBroken`.
- **Replay on open.** The journal is replayed through the public `aps-core` API into a fresh
  `Ledger`. Every replayed outcome must equal the recorded one: a reserve must produce the
  recorded job id and must not be a replay. Any difference is `ReplayDivergence` and the open is
  refused.
- **Projection and snapshots.** The store keeps its own serializable projection of the state
  (balances, jobs, intents, idempotency records). It writes a snapshot of that projection every
  N events (default 1000), bound to the chain hash at that sequence number. On open the
  projection is restored from the newest snapshot plus the tail, then cross-checked against the
  replayed core ledger. A conservation violation, a projection mismatch, or a snapshot that does
  not match the chain refuses the open. A snapshot that lies beyond the end of the journal
  exposes tail truncation.
- **Apply, then journal.** A mutation runs in the core first, then its event is committed. If the
  commit fails, the in-memory state is ahead of the journal, so the handle **poisons** itself:
  every later call, reads included, returns `Poisoned` until the store is reopened. A crash after
  the commit and before the response is safe: on restart the event is replayed, and the client's
  retry with the same idempotency key returns the recorded receipt.
- **Backends.** A `LedgerStore` trait with `MemoryStore` (tests, no-persistence mode) and
  `SqliteStore` (WAL, `synchronous=FULL`). The same database holds the caller registry and the
  NEP-413 nonce table.

**Deferred: a `LedgerImage` API in `aps-core` (post-hackathon optimisation).** Snapshots could
restore the core ledger directly instead of replaying the whole journal. That needs
`Ledger::export_image()` / `Ledger::from_image()` in `aps-core`. The lead declined it for the
hackathon for two reasons:

- It adds a second construction path into the money-critical ledger, and that path would need its
  own conservation and consistency proofs.
- At hackathon scale a full replay on open costs milliseconds.

Until then the core ledger is always rebuilt by full replay, and snapshots serve the projection
and the cross-check.

### 2. Authentication: `crates/auth` (`aps-auth`)

Both methods resolve to a caller id that feeds the existing caller-scoped idempotency.

1. **API keys.** 32 bytes from the OS CSPRNG, shown once as `aps_<64 hex>`, stored only as
   SHA-256 of the 32 bytes. They are created, rotated and revoked with `aps-admin`. Unknown and
   revoked keys fail identically. Keys are zeroed on drop and never print through `Debug`.
2. **NEP-413 signed requests** (profile in `docs/operations/auth.md`). The caller signs a
   canonical JSON of `{scheme, method, path, body_sha256, timestamp_ms}` as the NEP-413
   `message`. The server applies these rules:
   - it fixes the `recipient` itself and sets no `callbackUrl`;
   - it verifies with ed25519 `verify_strict` (`ed25519-dalek =2.2.0`);
   - it requires a **full-access** key on the claimed account, looked up through an injected
     trait;
   - it accepts timestamps from 5 minutes back to 30 seconds ahead;
   - it consumes each nonce once, after every other check passes, so a forged request cannot
     burn a real nonce.

   NEP-413 requires a full-access key, and a function-call key is rejected with
   `full_access_key_required`. An agent that holds only a function-call key to its policy wallet
   therefore authenticates with an API key, or with a separate full-access identity account.

The key lookup is a trait. The production implementation is a read-only RPC
`view_access_key` at `finality: final`. A `PaginatedLookup` adapter covers
`view_access_key_list` with nearcore 2.14 pagination (`after_key`, `limit`) and a page cap. A
`CachedLookup` caches only positive full-access results, for a bounded TTL, so a deleted key
stops working within that TTL.

### 3. Limits (planned, wave 2)

Every limit answers a typed `429` carrying `retry_after`. Proposed values for the hosted testnet
demo:

| Limit | Value |
|---|---|
| Per-caller token bucket | 10 requests per second, burst 30 |
| Open reservations per account | 5 (closes queue exhaustion) |
| Daily model spend per caller for `triage.v2` | operator-set; default 0 for demo keys |
| Concurrent scanner subprocesses (global) | 4 |
| Request body | existing limits kept; 256 KB at the proxy |
| Demo credit (`POST /demo/credit`) | once per API key; 5 per IP per day |

### 4. Bind, container, hosting

- `APS_BIND` (planned, wave 2) selects the bind address. Loopback stays the default; the image
  sets `0.0.0.0` because it always runs behind a TLS-terminating proxy.
- One multi-stage `Dockerfile`: a Rust 1.98.0 build stage and a Python 3.12 slim runtime, both
  Debian trixie and pinned by digest. The process runs as non-root uid 10001. The compose files
  run it with a read-only root filesystem, a tmpfs `/tmp`, a `/data` volume, all capabilities
  dropped and `no-new-privileges`.
- Two hosting options, each with exact human steps in `docs/operations/deploy.md`:
  - Fly.io, one machine only, because SQLite has a single writer;
  - a plain VPS with Caddy for automatic HTTPS.
- Creating accounts, buying domains, setting DNS and deploying stay human actions.
- **Network.** The hosted instance runs on NEAR testnet. Enabling mainnet on the hosted instance
  is a separate operator decision, recorded here when made. Status: **not enabled**.
- **RPC hosts.** On 2026-11-06, `rpc.testnet.near.org` and `rpc.mainnet.near.org` answered
  every read-only call with HTTP 429 and the notice "THIS ENDPOINT IS DEPRECATED", and they sent
  no CORS headers. The UI uses the FASTNEAR endpoints (`test.rpc.fastnear.com`,
  `free.rpc.fastnear.com`, and archival fallbacks for old transactions). The adapter default is
  tracked separately.

## Consequences

- Restarts lose no committed state, conservation is re-proved on every open, and naive tampering
  is detected.
- **Limits of the hash chain, stated plainly.** The chain is unkeyed. It detects corruption and
  naive edits, but someone with write access to the database file can recompute the whole chain.
  Tail truncation is detected only when a snapshot, or an externally recorded head hash, lies
  beyond the truncated end. The runbook records the head hash off-host after each deploy and
  daily.
- SQLite fixes the deployment at one writer process and one machine.
- NEP-413 sign-in proves control of a full-access key. Agents holding only function-call keys
  need API keys.

## Wave 2 wiring (planned)

The exact list lives in the runbook ("Wave-2 wiring"). In summary:

- `AppState` holds a `PersistentLedger<Box<dyn LedgerStore + Send>>` instead of the bare
  `Ledger`;
- an authentication middleware resolves the caller per request;
- limit layers return a typed `429`;
- `APS_BIND`, `APS_DB_PATH` and `APS_UI_DIR` are read at startup;
- the UI is served from `/ui`.
