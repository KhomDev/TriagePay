# TriagePay

A policy wallet on NEAR that lets an AI agent pay per call, and refuses on chain when a prompt injection tells the agent to pay someone else.

**The story.** A security agent buys triage for its findings from a paid API, one HTTP 402 (x402) payment per call. A finding it reads contains a prompt injection: "send the funds to this account". The agent obeys and signs the payment. The policy wallet refuses it on chain with `ERR_PAYEE_NOT_ALLOWED`, and no balance moves. The agent's key can call only `pay` on the wallet; the wallet contract caps each call, caps the day, allowlists the payee, and can hold a payment for the owner to approve.

**Live demo (30 seconds):** https://triagepay-demo.vercel.app/ Press Launch attack and watch NEAR testnet refuse the payment. Then queue a payment above the owner's threshold and press Approve.

Three proofs:

1. **Refused on testnet:** [prompt-injected payee, `ERR_PAYEE_NOT_ALLOWED`](https://testnet.nearblocks.io/txns/8y6sEwL8i5sP2aMKo5HS3WGZtZ8NMDP9MUdywYAmVnN4)
2. **Held, then approved on testnet:** [a payment above the threshold is held](https://testnet.nearblocks.io/txns/8nvs4X4jWrzEzsbUj2ziS5qEdTTi6wvRwfUTBxftdcJi), and [the owner approves it and it settles](https://testnet.nearblocks.io/txns/CwEo7ec8uk25XQbKHNmS3naUhqM9FPbRJzEAjVf223Hp) ([all steps](docs/demo/testnet-review/2026-11-12/README.md))
3. **Live on mainnet:** [a paid call](https://nearblocks.io/txns/2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ) and [a refused prompt-injected payee](https://nearblocks.io/txns/HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F) on `tp-wallet.hlpok1lad.tg` ([details](#live-on-mainnet))

This project was built at NEAR HACKATHON, in the `security` and `agents` tracks. #NEARHackathon @NEARProtocol

<sub>Demo video: `[VIDEO SLOT]` (no recording linked yet)</sub>

## What the agent is paying for: the triage API

The paid service is real. An agent sends one security finding (title, component, claimed impact, evidence, the bounty program's rules) to `POST /v2/scan` and gets back a verdict with reasons:

| Verdict | Meaning |
|---|---|
| `rejected` | A deterministic gate failed; the reasons name the category |
| `held` | A gate flagged something a person should look at first |
| `needs_evidence` | The claim may be real, but the evidence is missing or only a placeholder |
| `supported` | No deterministic objection |

Two stages produce it:

- **Deterministic scanner (Stage A).** Nine gates, one per public finding category, compiled from a versioned ruleset. The same finding always gets the same verdict. Text inside a finding that tries to instruct the reader is flagged and never changes the outcome.
- **NEAR AI judge (Stage B, optional).** A model on NEAR AI Cloud gives a second opinion with a short rationale. It is advisory: it can lower a verdict, never raise it.

Why an agent would pay for this per call: a bug-hunting or code-review agent produces many candidate findings, and most should not reach a bounty program or a human reviewer. The triage call filters them first and says why. With x402 there is no account or API key to set up: the 402 answer names the asset, the payee, the amount and a memo, and the agent pays and retries. That payment is exactly the step a prompt injection goes after, which is why the agent pays through the policy wallet.

Internals (rule evaluators, the Stage B prompt and schema, the combiner, the knowledge compiler that builds the ruleset): [docs/engine/overview.md](docs/engine/overview.md).

## Why TriagePay and not another agent spend guard

These are the facts I can point to. I have not benchmarked other tools.

- **The limits are on chain, not in the agent.** The agent holds a NEAR function-call key that can call only `pay` on the wallet. Caps, the payee allowlist, pause and revoke live in the contract, so a prompt-injected or stolen key cannot switch them off. Owner methods need 1 yoctoNEAR, which no function-call key can attach.
- **The server checks the payment from the token contract's own events.** An x402 payment counts only when the token contract's own NEP-141 `ft_transfer` event, in a finalized receipt, pays the gateway with the server-issued memo. Logs from any other contract are ignored. This runs on testnet ([how](#how-the-server-knows-it-was-paid)).
- **An owner review queue on chain.** A new payee or an amount above a threshold is held in the contract until the owner approves or rejects it. Run on testnet: [held](https://testnet.nearblocks.io/txns/8nvs4X4jWrzEzsbUj2ziS5qEdTTi6wvRwfUTBxftdcJi), [approved](https://testnet.nearblocks.io/txns/CwEo7ec8uk25XQbKHNmS3naUhqM9FPbRJzEAjVf223Hp), [rejected](https://testnet.nearblocks.io/txns/CcUmBvhQPBvtPJ9pg4NF225U4XfaJEmwjLEZV9gCAaWR).
- **Live on mainnet** with real (tiny) wNEAR amounts: paid, refused, revoked ([below](#live-on-mainnet)).
- **A reusable SDK.** `packages/triagepay-sdk` is a one-line policy-wallet payer, an x402 helper and a NEAR AI tool (9 tests).
- **A claims audit.** Every testable claim in the docs is mapped to its evidence, with what is limited or open said plainly: [docs/claims-audit.md](docs/claims-audit.md).

## Live on mainnet

I deployed the policy wallet on NEAR mainnet and ran it with real (tiny) wNEAR amounts on 2026-11-12. The wallet is still live. It is the build before the owner review queue.

- Wallet contract: [`tp-wallet.hlpok1lad.tg`](https://nearblocks.io/address/tp-wallet.hlpok1lad.tg) (0 access keys; owner `hlpok1lad.tg`; token `wrap.near`; payee allowlist `[gateway.hlpok1lad.tg]`)
- Agent: [`tp-agent2.hlpok1lad.tg`](https://nearblocks.io/address/tp-agent2.hlpok1lad.tg), whose only key is a function-call key for `pay` on the wallet
- **Paid on mainnet:** [allowlisted payee, inside the cap, `pay_settled`](https://nearblocks.io/txns/2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ)
- **Refused on mainnet:** [prompt-injected payee, `ERR_PAYEE_NOT_ALLOWED`, no balance change](https://nearblocks.io/txns/HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F)
- **Revoked on mainnet:** [owner `revoke_agent`](https://nearblocks.io/txns/CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh), then [the same key refused, `ERR_AGENT_INACTIVE`](https://nearblocks.io/txns/C1BnCCrEMYfxhirf283GfGr2sfeBMaZ88fimxQwoKt2d)

Every transaction, the costs, and two setup mistakes I hit are in [`docs/demo/mainnet-e2e/2026-11-12/`](docs/demo/mainnet-e2e/2026-11-12/README.md); the script is `scripts/mainnet/mainnet-proof.mjs`. The x402 receipt check runs on testnet only: `wrap.near` emits no NEP-141 events for the verifier to read.

A later mainnet run is in progress on another machine. Its notes will land under `docs/demo/mainnet-*`. I have not written a result for that run here. The 2026-11-12 transactions above are the mainnet evidence that exists today.

## Hosted demo

`demo/live` is the judge page at https://triagepay-demo.vercel.app/. Press Launch attack and the server signs `pay` on NEAR testnet. The page shows the attack path, live spend, a receipt, and a replay URL. The browser never sees a key.

What is on the page, and what it needs:

| Feature | Status | Env |
|---|---|---|
| Launch attack, scenarios, payee risk, Revoke and Restore | Live on testnet when the two signing keys are set | `TRIAGEPAY_AGENT_SECRET_KEY`, `TRIAGEPAY_OWNER_SECRET_KEY` |
| Owner review (hold, Approve, Reject) | Live on the testnet wallet. The threshold is shown in wNEAR | the owner key |
| Drain Me | A visitor's text runs through the agent and the wallet. I do not sign a payment the wallet would settle | the agent key; per-address cap `TRIAGEPAY_DRAIN_IP_LIMIT` (default 3) |
| Scripted agent | Default. The page says `scripted` | none |
| NEAR AI agent | Used when the key is set. If the call fails, I fall back and still say `scripted` | `NEARAI_API_KEY`, optional `NEARAI_MODEL` |
| Receipts and the PNG share card | No secret. Share text includes the hackathon line | none |
| Attack replay | `/replay/<tx>` plays about ten seconds. A stored narrative is used when this server signed the transaction. Otherwise I rebuild the payee, the amount, the error code, and the demo sentence from the transaction | shared store optional |
| Honeypot payee | The injected account is the trap. The list is the agent's refused `pay` calls to that account, read from the chain index and the RPC. The deployed wallet has no attacker registry of its own | optional `TRIAGEPAY_HONEYPOT_ACCOUNT` (defaults to the injected account) |
| Telegram approval | A hold can message the owner with Approve and Reject. Off until the three variables are set | `TELEGRAM_BOT_TOKEN`, `TELEGRAM_OWNER_CHAT_ID`, `TELEGRAM_WEBHOOK_SECRET` |
| Kill switch | An expiring single-use link and a QR code. Off until the secret is set. Revoke on the page still works | `KILL_SWITCH_SECRET` |
| Natural-language policy | Preview always. Apply signs `set_agent` / `set_review_policy` only when the flag is `1`. It is off on the public demo | `TRIAGEPAY_POLICY_APPLY=1` to apply; `NEARAI_API_KEY` optional, otherwise the parser |
| Agent passport | `/agent/<account>` and `GET /api/passport?account=`. Limits from the wallet. Rows from the indexer. Error codes only when this server's journal has them | none |

The demo accounts on testnet: wallet `tpd-e947e7-wallet.testnet` (with the owner review queue), agent `tpd-e947e7-agent.testnet`, owner `tpd-e947e7-owner.testnet`. The agent's only key is the pay-only function-call key. `/api/health` shows which wallet the page is using.

An allowlisted payee shows warn when the account is under seven days old. The demo payee was created for the page, so warn is expected; warn does not move money. If a visitor leaves the agent revoked, the next Launch restores it before the attack, and a daily cron calls the same restore.

The same wallet also refused [an over-limit payment (`ERR_PER_CALL_LIMIT`)](https://testnet.nearblocks.io/txns/BMoHjQXgQasLxn4wZspzTQU9g1rBjbqThhZQevknCZ7s) and, after [a revoke](https://testnet.nearblocks.io/txns/5qZ5yQAPTm5dFg7i455HvsrgBNk8bTbbCou6zq4461as), [a revoked agent (`ERR_AGENT_INACTIVE`)](https://testnet.nearblocks.io/txns/3iHytU9nkYuqPh74its9TPW3MwQoUJeaPQg1nbGGyQTZ).

Running or deploying it yourself (environment variables, rate limits, the shared store, setup, and how the hosted keys were moved): [docs/demo/hosted.md](docs/demo/hosted.md).

## Chain Signatures

[`v1.signer-prod.testnet` signed a Base Sepolia USDC transfer](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM) for the owner account, and the signature recovers to the derived address `0x508e984fda3bc37d8c33bfb5098441fd2169fcc8`. It was not broadcast, and the wallet contract does not call `sign` yet ([docs/contracts/chain-signatures.md](docs/contracts/chain-signatures.md)).

## The problem

An agent that holds a normal NEAR key can empty its account the first time a prompt injection tells it to. A function-call key cannot move tokens either: it cannot attach the 1 yoctoNEAR that NEP-141 `ft_transfer` requires. So the agent needs a contract between its key and the money.

## Try it

### A. Offline, no keys, about two minutes

Needs Rust (stable) and Python 3.12.

| Do | See |
|---|---|
| `cargo test --workspace` | `226 passed`, `3 ignored` (the ignored three read mainnet RPC) |
| Terminal 1: `cargo run -p aps-api --bin aps-api` | `aps-api listening on http://127.0.0.1:8080 (payment provider: mock)` |
| Terminal 2: `python scripts/x402-demo.py` | Step 1: `HTTP 402` and the decoded payment requirement. Step 2: `HTTP 200`, `verdict: needs_evidence`, the content hash the payment settled against. Step 3: the same proof again returns the identical response. Step 4: the proof reused for another finding is `proof_already_consumed` |
| `./scripts/demo.sh` (or `demo.ps1`) | The prepaid flow without x402: fund, reserve, scan, settle, reject, refund; final balances conserve |

The mock provider accepts any transaction hash on a development-only network (`aps:mock`). It shows the HTTP exchange and the ledger, not payment verification.

To add the NEAR AI judge without a key, start the API with `APS_SCAN_SKU=triage.v2` and `APS_TRIAGE_CASSETTES=triage-model/cassettes`: the walkthrough then also prints the judge's opinion and rationale, replayed from recorded replies. With `APS_LIVE_MODEL=1` and `NEARAI_API_KEY` set instead, the call goes to NEAR AI Cloud (about USD 0.0002 per call). Details: [docs/engine/overview.md](docs/engine/overview.md).

### B. Read the testnet run, no setup

[docs/demo/transcript-2026-11-11.md](docs/demo/transcript-2026-11-11.md) is the unedited output of the agent demo on NEAR testnet. Every step links to its transaction. In order:

1. The wallet account has 0 access keys. The agent's key is `FunctionCall{receiver: wallet, methods: [pay]}`.
2. The agent pays for four triage calls. Each is a 402, a wallet `pay`, and a proof the server accepts only after reading the token contract's transfer event. Verdicts: `needs_evidence`, `supported`, `supported` with embedded instructions flagged, `rejected`.
3. A finding tells the agent to send a token to another account. The agent obeys; the wallet answers `ERR_PAYEE_NOT_ALLOWED` and no balance moves.
4. One atomic unit over the per-call limit: `ERR_PER_CALL_LIMIT`.
5. A fifth call the same day: `ERR_DAILY_LIMIT`.
6. The agent calls the owner method `set_agent` to raise its own limit. The node rejects the transaction: a function-call key cannot attach the required deposit, and the key does not list the method.
7. The owner calls `revoke_agent`. The agent's key still exists and its next `pay` fails with `ERR_AGENT_INACTIVE`.

### C. Re-run it on testnet, about ten minutes

Needs Node 22.18 or later, `cargo-near` 0.21.1 with the `wasm32-unknown-unknown` target, and the key file of a testnet account holding about 6 NEAR (`{"account_id": "...", "secret_key": "ed25519:..."}`, kept outside the repository). The script creates throwaway accounts and never touches mainnet.

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

`../demo-run/transcript.md` then reads like the file in B, with new transaction hashes. Details: [agents/demo/README.md](agents/demo/README.md).

## What NEAR does here

| NEAR feature | Use | Proof |
|---|---|---|
| Function-call access keys | The key the agent uses to pay: one contract, one method, no deposit. On the hosted agent account it is the only key | Transcript steps `keys_view`, `self_escalation`; [docs/contracts/policy-wallet.md](docs/contracts/policy-wallet.md) testnet steps 11-24; [docs/demo/hosted.md](docs/demo/hosted.md) for the hosted keys |
| A contract as the spending policy | Per-call limit, per-UTC-day limit, payee allowlist, pause, revoke, and an owner queue for a new payee or an amount above a threshold; owner methods need 1 yoctoNEAR, which no function-call key can attach | `contracts/policy-wallet`: 78 unit tests and 18 sandbox tests, 3 property tests in `crates/policy-core`; transcript refusals |
| Chain Signatures | One testnet `sign` request on `v1.signer-prod.testnet` for an EIP-1559 USDC transfer on Base Sepolia; the signature recovers to the address derived for (owner account, `base-sepolia-1`). Not broadcast, and the wallet does not call `sign` yet | [docs/contracts/chain-signatures.md](docs/contracts/chain-signatures.md); [sign transaction](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM); SDK tests reuse the recorded values |
| NEP-141 and NEP-297 events | Payment is an `ft_transfer`; the server accepts it only when the token contract's own `ft_transfer` event, in a finalized receipt, pays the gateway with the server-issued memo | `crates/near-adapter/src/verify.rs`; 21 receipt-fixture tests, one a real mainnet capture; transcript |
| x402 v2 over NEAR | A client-submitted payment proof (`near-tx-proof`) that works from a policy wallet. The upstream NEAR binding needs a full-access key; this one does not. A proposed profile, not upstream | [docs/protocols/x402-near.md](docs/protocols/x402-near.md); 16 x402 flow tests; transcript |
| NEP-413 signed messages | Login-message verification for future caller authentication | `crates/auth`, 25 tests. A library: the API does not use it yet |
| NEAR AI Cloud | The optional Stage B judge (`APS_SCAN_SKU=triage.v2`) calls a model the catalogue lists as TEE-verifiable; it is advisory and can never raise a verdict. The hosted demo separately checks that an attestation report echoes a fresh nonce. The Intel quote is not verified | Spike on 2026-11-12: 50 live calls, 5 models, USD 0.096 ([docs/engine/model-selection.md](docs/engine/model-selection.md)); recorded replies in `triage-model/cassettes`, replayed by 6 tests. Paid route: no attestation (ADR-0011). Demo check: [docs/engine/attestation.md](docs/engine/attestation.md) |

## What the agent's key can and cannot do

- It can call `pay(token, receiver, amount, memo)` on one wallet, with no deposit. The wallet checks, in order: not paused, agent active, token allowlisted, payee allowlisted, amount within the per-call limit, day total within the daily limit. Then the wallet itself sends the `ft_transfer`. With owner review on, a new payee or an amount above the threshold is queued instead of transferred ([docs/contracts/review-policy.md](docs/contracts/review-policy.md)).
- On the hosted agent account (`tpd-e947e7-agent.testnet`) this is the only key. [docs/demo/hosted.md](docs/demo/hosted.md) has the deletion transaction.
- It cannot call any owner method, transfer NEAR, add or delete keys, or deploy code. Those need a deposit or a full-access key.
- Stolen, the pay-only key can spend at most the daily limit per UTC day, to allowlisted payees only, until the owner revokes the agent or pauses the wallet. Across a UTC midnight that is up to two daily limits within minutes.
- It carries a gas allowance. When the allowance runs out the key stops working until the account's full-access key replaces it. An account that holds only function-call keys can lock itself out this way. The hosted agent is such an account.
- Revocation happens at the wallet (`revoke_agent`). The wallet owner cannot delete a key on the agent's account; the demo shows key deletion as a separate step by the agent account.
- One wallet holds one token. Limits are integers in that token's atomic units.
- The owner key is not limited: it can withdraw everything. The wallet account itself has no keys and no upgrade method.

## How the server knows it was paid

1. `POST /v2/scan` without payment creates a deposit intent and answers 402. The requirement names the asset, the payee, the exact amount and a memo that ends in a 128-bit random nonce.
2. The agent pays with that memo, directly or through the wallet, and retries with the transaction hash.
3. The server reads the transaction with `wait_until: FINAL` and looks for a successful receipt executed by the asset contract, carrying an `ft_transfer` call with 1 yoctoNEAR and exactly one `nep141` event to the payee with the memo. Events logged by any other contract are ignored. `ft_transfer_call` is refused because it can be refunded.
4. The exact amount funds the intent once. A wrong amount, a second payment or a late one is recorded as an orphan for an operator-approved refund and never becomes credit.
5. The ledger reserves the price, runs the scan, and settles against the report's content hash. The same proof and request replay the stored response; anything else is refused.

Money is integer-only (u128 atomic units). Every balance satisfies `deposited == available + reserved + settled`, checked after every operation by a randomized test.

## Status

| | |
|---|---|
| Run on testnet | Policy wallet (28-step run on 2026-11-06, and the agent demo on 2026-11-11); the owner-review wallet on 2026-11-12 (held, approved, rejected) and again from the production page on 2026-11-13; one Chain Signatures request on 2026-11-12, signed and not broadcast; x402 round trips paid through the wallet and directly; receipt verification |
| Run on mainnet | The policy wallet on 2026-11-12: one paid call, one refused payee, revoke, one revoked-agent refusal, in wNEAR ([Live on mainnet](#live-on-mainnet)). Earlier, one native-USDC deposit verified on 2026-10-21 by the argument-based verifier ([docs/integrations/near.md](docs/integrations/near.md)). The x402 receipt check has not run on mainnet, and the review queue is not deployed there |
| Run against NEAR AI Cloud | Stage B: a 50-call spike on 2026-11-12 and the paid route in `triage.v2` mode. Every verdict in the 2026-11-11 testnet transcript comes from Stage A alone; the run predates the working key |
| Built, tested offline only | The x402 facilitator endpoints, the SQLite journal (`crates/store`), NEP-413 and API keys (`crates/auth`), the static UI (`ui/`), the Dockerfile |
| Not wired | The API keeps its ledger in memory, serves one development caller with no authentication, binds loopback only, and does not serve the UI |
| Run in CI only | The 18 sandbox tests in `contracts/tests` (near-workspaces, a local NEAR node) pass on GitHub Actions, Linux. The sandbox has no Windows build |
| Hosted judge demo | `demo/live`: Vercel static page plus serverless functions on testnet. Signing keys are environment variables. The cross-instance rate-limit store is not connected yet (`sharedLimits: false`) |
| Cut | Attested verdicts on the paid route (ADR-0011); the demo checks a nonce echo and does not verify the Intel quote. Also cut: NEAR Intents deposits and a measured accuracy figure. No accuracy is claimed anywhere |

Known limits of the paid route: no rate limiting; the server trusts its RPC endpoint; on `/v2/scan` the envelope is validated after payment (an invalid one keeps the payment claimed and can be retried with a corrected body); The hosted demo calls a NEAR AI model only when `NEARAI_API_KEY` is set, and otherwise uses a scripted agent and says so.

## Hackathon window and what is reused

The first commit is dated 2026-10-20. The rules count code from the announcement on 2026-10-19 (Q-001), so no code here predates the window. `git log --reverse` shows the whole history.

Libraries: axum, tokio, serde, ureq, base64, getrandom, ed25519-dalek, sha2, bs58, rusqlite (bundled SQLite), proptest; near-sdk 5.29.1, near-contract-standards and near-workspaces for the contracts; near-api-js 7.3.1 for the clients and testnet scripts. The Python packages use the standard library only. The x402 work follows the public x402 v2 specification; the `near-tx-proof` method is my own proposal.

I am one developer and I wrote this with AI coding agents (Claude Code). The commits carry the co-author trailer.

## Tests

All counts are from one run on 2026-11-13 on the merged main, except the sandbox, which needs Linux and runs in CI.

| Suite | Command | Result |
|---|---|---|
| Rust workspace | `cargo test --workspace` | 226 passed, 3 ignored |
| Contracts | `cd contracts && cargo test` | policy-wallet 78 passed, test-token 3 passed; sandbox 18 passed in the contracts workflow on GitHub Actions (Linux) |
| Scanner | `cd scanner && python -m pytest -q` | 319 passed |
| Knowledge compiler | `cd knowledge-compiler && python -m pytest -q` | 71 passed, 1 skipped |
| Model judge (offline, with cassette replay) | `cd triage-model && python -m pytest -q` | 73 passed |
| OpenAPI and Python client | `python -m pytest tests -q` | 14 passed |
| UI | `python -m pytest ui/tests -q` | 17 passed |
| TypeScript client | `cd clients/typescript && npm ci && npm test` | 7 passed |
| Hosted demo | `cd demo/live && npm test` | 68 passed on 2026-11-13 |
| Policy-wallet SDK | `cd packages/triagepay-sdk && npm test` | 9 passed |

The claim-by-claim record is [docs/claims-audit.md](docs/claims-audit.md).

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
triage-model/            Stage B model judge on NEAR AI Cloud, recorded cassettes
knowledge-compiler/      compiles rule sources into hash-addressed rulesets
clients/                 x402 reference clients (TypeScript, Python)
agents/demo/             the testnet agent demo (full x402 loop, throwaway accounts)
demo/live                hosted judge demo (Vercel): attack, replay, honeypot list, policy preview, passport
packages/triagepay-sdk   one-line policy-wallet payer, x402 helper, NEAR AI tool
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
| `APS_SCANNER_PYTHON`, `APS_SCANNER_DIR`, `APS_SCANNER_TIMEOUT_SECS` | discovered, discovered, 30 | Scanner subprocess (use 120 with a live model) |
| `APS_SCAN_SKU` | scan.v1 | `triage.v2` adds the Stage B model judge |
| `APS_LIVE_MODEL`, `NEARAI_API_KEY` | unset | Both set: live Stage B calls to NEAR AI Cloud |
| `APS_TRIAGE_CASSETTES` | unset | Replay Stage B from a cassette directory, no key |
| `APS_TRIAGE_MODEL_ID` | z-ai/glm-5.3-flash | Stage B model |

## More

[Architecture](docs/architecture.md) |
[threat model](docs/threat-model.md) |
[wallet](docs/contracts/policy-wallet.md) |
[x402 profile](docs/protocols/x402-near.md) |
[engine](docs/engine/overview.md) |
[hosted demo](docs/demo/hosted.md) |
[decisions](docs/decisions/)

## License

MIT. See [LICENSE](LICENSE).
