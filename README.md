# TriagePay

An AI agent pays per API call from a wallet contract on NEAR that caps each
call, caps the day and allowlists the payees. The agent's key can do one
thing: call `pay` on that wallet.

The paid API is a triage service for security findings: an agent submits a
finding and gets a deterministic verdict (`supported`, `needs_evidence`,
`held` or `rejected`) with the reasons. Payment is HTTP 402 (x402 v2) settled
by a NEAR transaction that the server verifies from the token contract's own
receipt.

Built for NEAR HACKATHON, tracks `security` and `agents`.

## The problem

An agent that holds a normal NEAR key can empty its account the first time a
prompt injection tells it to. A function-call key cannot move tokens either:
it cannot attach the 1 yoctoNEAR that NEP-141 `ft_transfer` requires. So the
agent needs a contract between its key and the money.

## Try it

### A. Offline, no keys, about two minutes

Needs Rust (stable) and Python 3.12.

| Do | See |
|---|---|
| `cargo test --workspace` | `224 passed`, `3 ignored` (the ignored three read mainnet RPC) |
| Terminal 1: `cargo run -p aps-api --bin aps-api` | `aps-api listening on http://127.0.0.1:8080 (payment provider: mock)` |
| Terminal 2: `python scripts/x402-demo.py` | Step 1: `HTTP 402` and the decoded payment requirement. Step 2: `HTTP 200`, `verdict: needs_evidence`, the content hash the payment settled against. Step 3: the same proof again returns the identical response. Step 4: the proof reused for another finding is `proof_already_consumed` |
| `./scripts/demo.sh` (or `demo.ps1`) | The prepaid flow without x402: fund, reserve, scan, settle, reject, refund; final balances conserve |

The mock provider accepts any transaction hash on a development-only network
(`aps:mock`). It shows the HTTP exchange and the ledger, not payment
verification.

### B. Read the testnet run, no setup

[docs/demo/transcript-2026-11-11.md](docs/demo/transcript-2026-11-11.md) is
the unedited output of the agent demo on NEAR testnet. Every step links to
its transaction. In order:

1. The wallet account has 0 access keys. The agent's key is
   `FunctionCall{receiver: wallet, methods: [pay]}`.
2. The agent pays for four triage calls. Each is a 402, a wallet `pay`, and a
   proof the server accepts only after reading the token contract's transfer
   event. Verdicts: `needs_evidence`, `supported`, `supported` with embedded
   instructions flagged, `rejected`.
3. A finding tells the agent to send a token to another account. The agent
   obeys; the wallet answers `ERR_PAYEE_NOT_ALLOWED` and no balance moves.
4. One atomic unit over the per-call limit: `ERR_PER_CALL_LIMIT`.
5. A fifth call the same day: `ERR_DAILY_LIMIT`.
6. The agent calls the owner method `set_agent` to raise its own limit. The
   node rejects the transaction: a function-call key cannot attach the
   required deposit, and the key does not list the method.
7. The owner calls `revoke_agent`. The agent's key still exists and its next
   `pay` fails with `ERR_AGENT_INACTIVE`.

### C. Re-run it on testnet, about ten minutes

Needs Node 22.18 or later, `cargo-near` 0.21.1 with the
`wasm32-unknown-unknown` target, and the key file of a testnet account
holding about 6 NEAR (`{"account_id": "...", "secret_key": "ed25519:..."}`,
kept outside the repository). The script creates throwaway accounts and
never touches mainnet.

```sh
npm ci --prefix clients/typescript
(cd contracts/policy-wallet && cargo near build non-reproducible-wasm --locked --no-abi)
(cd contracts/test-token && cargo near build non-reproducible-wasm --locked --no-abi)
node agents/demo/demo.mjs setup --out-dir ../demo-run --funder-key ../funder.json
# setup prints the line that starts the API with the new gateway and token ids
node agents/demo/demo.mjs run    --out-dir ../demo-run --api http://127.0.0.1:8080
node agents/demo/demo.mjs direct --out-dir ../demo-run --api http://127.0.0.1:8080
node agents/demo/demo.mjs revoke --out-dir ../demo-run --api http://127.0.0.1:8080
```

`../demo-run/transcript.md` then reads like the file in B, with new
transaction hashes. Details: [agents/demo/README.md](agents/demo/README.md).

## What NEAR does here

| NEAR feature | Use | Proof |
|---|---|---|
| Function-call access keys | The agent's only key: one contract, one method, no deposit | Transcript steps `keys_view`, `self_escalation`; [docs/contracts/policy-wallet.md](docs/contracts/policy-wallet.md) testnet steps 11-24 |
| A contract as the spending policy | Per-call limit, per-UTC-day limit, payee allowlist, pause, revoke; owner methods need 1 yoctoNEAR, which no function-call key can attach | `contracts/policy-wallet`: 65 unit tests, 3 property tests in `crates/policy-core`; transcript refusals |
| NEP-141 and NEP-297 events | Payment is an `ft_transfer`; the server accepts it only when the token contract's own `ft_transfer` event, in a finalized receipt, pays the gateway with the server-issued memo | `crates/near-adapter/src/verify.rs`; 21 receipt-fixture tests, one a real mainnet capture; transcript |
| x402 v2 over NEAR | A client-submitted payment proof (`near-tx-proof`) that works from a policy wallet. The upstream NEAR binding needs a full-access key; this one does not. A proposed profile, not upstream | [docs/protocols/x402-near.md](docs/protocols/x402-near.md); 15 flow tests; transcript |
| NEP-413 signed messages | Login-message verification for future caller authentication | `crates/auth`, 25 tests. A library: the API does not use it yet |
| NEAR AI Cloud | The optional Stage B model judge targets a TEE-listed model | Not proven. See Status |

## What the agent's key can and cannot do

- It can call `pay(token, receiver, amount, memo)` on one wallet, with no
  deposit. The wallet checks, in order: not paused, agent active, token
  allowlisted, payee allowlisted, amount within the per-call limit, day total
  within the daily limit. Then the wallet itself sends the `ft_transfer`.
- It cannot call any owner method, transfer NEAR, add or delete keys, or
  deploy code. Those need a deposit or a full-access key.
- Stolen, it can spend at most the daily limit per UTC day, to allowlisted
  payees only, until the owner revokes the agent or pauses the wallet. Across
  a UTC midnight that is up to two daily limits within minutes.
- It carries a gas allowance. When the allowance runs out the key stops
  working until the account's full-access key replaces it. An account that
  holds only function-call keys can lock itself out this way.
- Revocation happens at the wallet (`revoke_agent`). The wallet owner cannot
  delete a key on the agent's account; the demo shows key deletion as a
  separate step by the agent account.
- One wallet holds one token. Limits are integers in that token's atomic
  units.
- The owner key is not limited: it can withdraw everything. The wallet
  account itself has no keys and no upgrade method.

## How the server knows it was paid

1. `POST /v2/scan` without payment creates a deposit intent and answers 402.
   The requirement names the asset, the payee, the exact amount and a memo
   that ends in a 128-bit random nonce.
2. The agent pays with that memo, directly or through the wallet, and
   retries with the transaction hash.
3. The server reads the transaction with `wait_until: FINAL` and looks for a
   successful receipt executed by the asset contract, carrying an
   `ft_transfer` call with 1 yoctoNEAR and exactly one `nep141` event to the
   payee with the memo. Events logged by any other contract are ignored.
   `ft_transfer_call` is refused because it can be refunded.
4. The exact amount funds the intent once. A wrong amount, a second payment
   or a late one is recorded as an orphan for an operator-approved refund
   and never becomes credit.
5. The ledger reserves the price, runs the scan, and settles against the
   report's content hash. The same proof and request replay the stored
   response; anything else is refused.

Money is integer-only (u128 atomic units). Every balance satisfies
`deposited == available + reserved + settled`, checked after every operation
by a randomized test.

## Status

| | |
|---|---|
| Run on testnet | Policy wallet (28-step run on 2026-11-06, and the agent demo on 2026-11-11 with the current build); x402 round trips paid through the wallet and directly; receipt verification |
| Run on mainnet | One native-USDC deposit verified on 2026-10-21 by the earlier, argument-based verifier ([docs/integrations/near.md](docs/integrations/near.md)). Nothing since; the wallet is not on mainnet |
| Built, tested offline only | Triage engine v2 Stage A (deterministic gates) is what every verdict above comes from. Stage B (model judge on NEAR AI Cloud, `triage-model/`) has never run against the live service: every completion attempt was refused with HTTP 402 because the API key has no spending limit. Also: the x402 facilitator endpoints, the SQLite journal (`crates/store`), NEP-413 and API keys (`crates/auth`), the static UI (`ui/`), the Dockerfile |
| Not wired | The API keeps its ledger in memory, serves one development caller with no authentication, binds loopback only, and does not serve the UI |
| Never run | The 18 sandbox tests in `contracts/tests` (no Windows build of the sandbox; Linux CI runs them) |
| Cut | Attested verdicts (ADR-0011), NEAR Intents deposits, a hosted demo, a mainnet campaign, a measured accuracy figure. No accuracy is claimed anywhere |

Known limits of the paid route: no rate limiting; the server trusts its RPC
endpoint; on `/v2/scan` the envelope is validated after payment (an invalid
one keeps the payment claimed and can be retried with a corrected body); the
agent in the demo has no language model.

## Hackathon window and what is reused

The first commit is dated 2026-10-20. The rules count code from the
announcement on 2026-10-19 (Q-001), so no code here predates the window.
`git log --reverse` shows the whole history.

Libraries: axum, tokio, serde, ureq, base64, getrandom, ed25519-dalek, sha2,
bs58, rusqlite (bundled SQLite), proptest; near-sdk 5.29.1,
near-contract-standards and near-workspaces for the contracts; near-api-js
7.3.1 for the clients and testnet scripts. The Python packages use the
standard library only. The x402 work follows the public x402 v2
specification; the `near-tx-proof` method is my own proposal.

I am one developer and I wrote this with AI coding agents (Claude Code). The
commits carry the co-author trailer.

## Tests

| Suite | Command | Result |
|---|---|---|
| Rust workspace | `cargo test --workspace` | 224 passed, 3 ignored |
| Contracts | `cd contracts && cargo test` | policy-wallet 65, test-token 3; sandbox 18 not run |
| Scanner | `cd scanner && python -m pytest -q` | 319 passed |
| Knowledge compiler | `cd knowledge-compiler && python -m pytest -q` | 71 passed, 1 skipped |
| Model judge (offline) | `cd triage-model && python -m pytest -q` | 64 passed |
| OpenAPI and Python client | `python -m pytest tests -q` | 14 passed |
| UI | `python -m pytest ui/tests -q` | 17 passed |
| TypeScript client | `cd clients/typescript && npm ci && npm test` | 7 passed |

Counts were regenerated on 2026-11-11. The claim-by-claim record is
[docs/claims-audit.md](docs/claims-audit.md).

## Layout

```
contracts/policy-wallet  the spend-policy wallet (near-sdk)
contracts/test-token     NEP-141 test token used on testnet and in sandbox tests
crates/policy-core       the wallet's limit arithmetic, dependency-free
crates/core              integer-money prepaid ledger, dependency-free
crates/payment           provider trait, reconciliation, mock provider
crates/near-adapter      NEAR provider: intents, RPC, receipt verification
crates/api               HTTP API: prepaid routes, /v2/scan (x402), facilitator
crates/store, crates/auth  SQLite journal; NEP-413 and API keys (not wired)
scanner/                 triage engine: validation, Stage A gates, reports
triage-model/            Stage B model judge (offline tests only)
knowledge-compiler/      compiles rule sources into hash-addressed rulesets
clients/                 x402 reference clients (TypeScript, Python)
agents/demo/             the testnet agent demo
ui/                      static pages (not served by the API yet)
docs/                    ADRs, protocol profile, contract docs, transcript
```

## HTTP surface

| Route | Effect |
|---|---|
| `POST /v2/scan` | The paid scan behind HTTP 402 (x402 v2, `near-tx-proof`) |
| `GET /x402/supported`, `POST /x402/verify`, `POST /x402/settle` | Facilitator endpoints for third-party servers |
| `POST /payment/intents`, `GET /payment/intents/{id}`, `POST .../observations`, `POST .../refunds` | Prepaid funding from provider-observed deposits; refunds are proposals |
| `POST /reservations`, `GET /jobs/{id}`, `POST /jobs/{id}/start`, `/scan`, `/refunds` | Prepaid job lifecycle |
| `GET /accounts/{id}`, `GET /health` | Balances; liveness |
| `POST /accounts/{id}/deposits`, `POST /jobs/{id}/settlements` | Development only: 403 unless the mock provider is active |

Full contract: [docs/openapi.json](docs/openapi.json). One error shape
everywhere: `{"error":{"code","message"}}`.

| Variable | Default | Meaning |
|---|---|---|
| `APS_PORT` | 8080 | Listen port (loopback only) |
| `APS_PROVIDER` | mock | `mock` or `near` |
| `NEAR_NETWORK` | none | `mainnet` or `testnet` (near only) |
| `NEAR_GATEWAY_ACCOUNT` | none | Account that receives payments (near only) |
| `NEAR_ASSET_CONTRACT` | none | NEP-141 settlement asset; it must emit NEP-297 events (near only) |
| `NEAR_RPC_URL` | FASTNEAR free endpoint for the network | JSON-RPC endpoint |
| `APS_X402_PRICE_ATOMIC`, `APS_X402_MAX_TIMEOUT_SECS` | 1000000, 900 | Price of one call; requirement validity |
| `APS_SCANNER_PYTHON`, `APS_SCANNER_DIR`, `APS_SCANNER_TIMEOUT_SECS` | discovered, discovered, 30 | Scanner subprocess |

## More

[Architecture](docs/architecture.md) ·
[threat model](docs/threat-model.md) ·
[wallet](docs/contracts/policy-wallet.md) ·
[x402 profile](docs/protocols/x402-near.md) ·
[engine](docs/engine/overview.md) ·
[decisions](docs/decisions/)

## License

MIT. See [LICENSE](LICENSE).
