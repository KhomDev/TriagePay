# Operator runbook

Scope: running the TriagePay gateway as the hosted testnet demo. Deployment steps are in
`deploy.md`, and the authentication profile is in `auth.md`. The design and its limits are in
ADR-0012.

Status labels used below:

- **now**: works at the current commit.
- **planned (wave 2)**: needs the API wiring listed at the end of this file.

## Components

| Component | Where | Status |
|---|---|---|
| API (`aps-api`) | container entrypoint | now (in-memory, loopback bind) |
| Durable journal (`aps-store`) | `/data/aps.sqlite` | now as a library; planned (wave 2) in the API |
| Caller registry and NEP-413 nonces | same database | now as a library; planned (wave 2) in the API |
| Admin CLI (`aps-admin`) | `/usr/local/bin/aps-admin` in the image | now |
| Scanner | `/app/scanner`, run as a subprocess | now |
| UI | `/app/ui` (static) | now as files; planned (wave 2) at `/ui` |

## Serve the UI locally (now)

The UI is static and needs no build step:

```
cd ui
python -m http.server 8000
```

Open `http://127.0.0.1:8000/`. Without a reachable API, the pages run in mock-data mode and mark
every result MOCK DATA.

The Wallet policy and Pay on NEAR pages call public NEAR RPC directly in live mode. The Try it
and Ledger pages need the API on the same origin, because the API sends no CORS headers. That
works once the API serves `/ui` (wave 2). Until then, use mock mode or put a same-origin proxy in
front of both.

## Create, rotate and revoke callers (now)

Run these inside the container: `docker compose exec aps <command>`. On Fly.io, use
`fly ssh console -C "<command>"`.

```
aps-admin create-caller --db /data/aps.sqlite --caller agent-1
aps-admin list-callers  --db /data/aps.sqlite
aps-admin rotate-caller --db /data/aps.sqlite --caller agent-1
aps-admin revoke-caller --db /data/aps.sqlite --caller agent-1
```

- `create-caller` and `rotate-caller` print the new key on stdout exactly once. Hand it over
  through a private channel and do not paste it into tickets or chat logs. Only SHA-256 of the key
  is stored.
- `list-callers` prints the caller id, its status, the creation time and the first 8 hex digits
  of the key digest. It never prints a key.
- Rotation invalidates the old key at once. Revocation is permanent; to restore access, create a
  new caller id.

## Verify the journal (now)

```
aps-admin verify-journal --db /data/aps.sqlite
```

This prints the number of events, `head_seq`, `head_hash`, the newest snapshot, and the account
and job counts, and ends with `conservation: ok`. It exits non-zero when the chain is broken,
when replay diverges, or when conservation fails. Those are the same checks the service runs
before it starts.

**Record the head hash off-host.** After each deploy and once a day, copy `head_seq` and
`head_hash` to a place the server cannot write, such as a private note or a signed commit in
another repository. The chain is unkeyed, so this external anchor is what reveals a rewritten or
truncated journal later. To compare, check that the recorded hash matches the row with that
`seq`, and that `head_seq` has not gone backwards.

## Back up and restore SQLite (now)

```
aps-admin backup --db /data/aps.sqlite --out /data/backup-$(date +%Y%m%d).sqlite
```

The backup uses `VACUUM INTO`, which writes a consistent copy while the service runs. The output
file must not exist yet. Copy it off the host, then delete the local copy if space is tight.

To restore:

1. Stop the service.
2. Move the current `/data/aps.sqlite`, along with any `-wal` and `-shm` files, aside.
3. Copy the backup to `/data/aps.sqlite`.
4. Run `verify-journal` and compare its head with the head recorded off-host.
5. Start the service.

A restore rolls state back to the backup point. Events after that point are lost, so tell the
affected callers.

## Read logs (now)

- `docker compose logs -f aps`, or `fly logs`.
- The API logs one startup line and errors, never request bodies, keys or envelopes. The Caddy
  access log (VPS option) records method, path, status and timing; request headers are deleted
  and client IPs are masked.
- The healthcheck logs only the failure class, never a body.

## Incident checklist

1. **The service refuses to start** with `journal hash chain broken`, `replay diverged`,
   `snapshot ... does not match` or `conservation violated`:
   - do not delete or edit the database;
   - copy `/data` aside for analysis;
   - compare `verify-journal` output with the recorded head;
   - restore the last good backup only after deciding which events to re-credit.
2. **The store is poisoned** (`a journal write failed after the state changed`): the disk is
   full or the volume is read-only. Fix the volume, then restart. The restart replays the journal,
   and clients retry with their idempotency keys.
3. **A key has leaked:** revoke it with `revoke-caller`, create a new caller, and check
   `list-callers`. For a NEP-413 identity, the account owner deletes the key on chain, and it
   stops working within the lookup cache TTL.
4. **Abuse or a traffic spike:** the limits answer `429` once wired (planned, wave 2). Until then,
   scale nothing, because SQLite has a single writer. Block at the proxy or the provider firewall.
5. **NEAR RPC failures:** reads fail closed (no funding without verification). If the default
   provider fails, switch `NEAR_RPC_URL` to an alternative from the NEAR providers page.

## Threat model rows

Proposed additions to `docs/threat-model.md`. The numbering continues after the existing row 14.

| # | Threat | Vector | Control | Verified by |
|---|---|---|---|---|
| 15 | Journal tampering | edit, delete or reorder rows in the SQLite file | append-only triggers; SHA-256 hash chain verified on every open; full replay must reproduce every recorded outcome; projection cross-check; snapshots bound to the chain; off-host head anchor in the runbook. Limit: an attacker with file write access can recompute an unkeyed chain, and only the off-host anchor detects that | `crates/store/tests/journal.rs`: tampered row, deleted row, truncation behind a snapshot, tampered snapshot, forged replay divergence |
| 16 | Denial of service | request floods, queue exhaustion through open reservations, scanner process exhaustion, model-spend exhaustion | planned (wave 2): per-caller token bucket, open-reservation cap per account, global scanner semaphore, daily model-spend cap, typed `429` with `retry_after`. Now: body-size limits, a 256 KB proxy cap, container pids and memory limits | planned: 1000-request abuse test |
| 17 | Credential stuffing | guessing API keys | 256-bit random keys; lookup by SHA-256 digest only; identical errors for unknown and revoked keys; planned per-IP limit on `401`s | `crates/auth/src/apikey.rs` tests |
| 18 | Faucet abuse | farming demo credit | planned (wave 2): demo credit is mock credit, never on-chain funds; once per API key; per-IP daily cap; labelled as demo credit in the UI | planned: demo-credit tests |
| 19 | NEP-413 replay | resend a captured signed request | single-use nonce per account, stored in SQLite so it survives restarts; nonce consumed only after the signature and key checks; timestamp window of 5 min back and 30 s ahead; recipient fixed by the server; method, path and body hash inside the signed message | `crates/auth/tests/nep413.rs`: replayed nonce, nonce survives restart, forged request does not burn a nonce, binding of recipient, method, path and body |
| 20 | Function-call key misuse | authenticate with a key any dApp can request | NEP-413 requires a full-access key; `FunctionCall` permission is rejected | `a_function_call_key_is_not_enough` |

## Wave-2 wiring

The work below wires the wave-1 components into `aps-api`. All of it is in `crates/api/**`.

1. **Dependencies.** Add `aps-store` and `aps-auth` as path dependencies. For static files, add
   `tower-http` (feature `fs`) at an exact version.
2. **State.** Replace `ledger: Arc<tokio::sync::RwLock<Ledger>>` with
   `Arc<tokio::sync::Mutex<PersistentLedger<Box<dyn LedgerStore + Send>>>>`. Use a `Mutex`
   because the SQLite connection is `Send` but not `Sync`.
   - Drop the `job_ids` registry. `PersistentLedger::job_id(wire)` resolves ids and survives
     restarts.
   - Rebuild `intent_accounts` from `projection().intents`, and record intent changes with
     `record_intent_state`.
   - Note: the mock provider keeps its own intents in memory, so a pending mock intent does not
     survive a restart. Either journal provider observations or document it.
3. **Startup** (`main.rs`):
   - read `APS_BIND` (default `127.0.0.1`), `APS_DB_PATH` (absent means `MemoryStore`) and
     `APS_UI_DIR`;
   - open with `PersistentLedger::open(store, OpenOptions::default())`;
   - on error, print only the error class and exit 1;
   - log `head_seq` and `head_hash` once.
4. **Error mapping.**
   - `StoreError::Core(e)` keeps the current mapping;
   - `IdempotencyConflict` returns 409;
   - `Poisoned` and `Backend` return 503 `store_unavailable`;
   - the other open-time errors never reach a handler.
5. **Authentication middleware** (`axum::middleware::from_fn_with_state`):
   - `Authorization: Bearer aps_...` goes to `authenticate_api_key`;
   - `X-NEAR-Account`, `X-NEAR-Public-Key`, `X-NEAR-Signature`, `X-NEAR-Nonce` and
     `X-NEAR-Timestamp` go to `Nep413Verifier::verify(method, path_and_query, body, now_ms)`,
     with the body buffered under the existing size limit. The caller id becomes
     `near:<account_id>`;
   - otherwise the answer is `401`, except `/health` and `/ui`;
   - insert the caller id as a request extension and use it in place of `AppState::caller()`;
   - keep the caller registry and the nonce table on a second `SqliteStore` connection to the
     same file (WAL), so authentication never waits on the ledger lock.
6. **Caller isolation.** Accounts are not owned by callers today. Namespace the core account id
   as `<caller>/<account_id>` in the handlers, so one caller cannot read or spend another
   caller's account. Idempotency is already caller-scoped.
7. **Limits.** Add these, each returning a `429` body `{"error":{"code":"rate_limited",
   "message":...,"retry_after":<secs>}}` and a `Retry-After` header:
   - a per-caller token bucket;
   - an open-reservation count per account from the projection;
   - a `tokio::sync::Semaphore` around the scanner launch;
   - a daily model-spend counter for `triage.v2`.
8. **Routes.**
   - `POST /demo/credit`: once per caller, recorded as an idempotency entry with scope
     `demo.credit`, plus a per-IP limit;
   - `GET /ledger/status`: head, event count, conservation;
   - `GET /ui/*`: `ServeDir`, with `/` redirecting to `/ui/`.
9. **OpenAPI.** Add the `bearer` and NEP-413 header security schemes, the `429` responses, and
   the new routes.
10. **Demo script.** Let `scripts/demo.sh` accept `APS_BASE_URL` and skip `cargo run`, so it can
    target a container. `deploy/smoke.py` already covers that flow.
