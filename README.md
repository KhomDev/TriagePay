# TriagePay

An agent key that can sign freely will pay whoever a prompt tells it to.
TriagePay puts a wallet contract between that key and the money: the key can call only `pay`, and the contract caps each call, caps the day, and allowlists the payee.
A judge opens the hosted demo, runs a prompt-injected payment on NEAR testnet, and watches the wallet refuse it.

**Demo video:** `[VIDEO SLOT]` No recording is linked in this repository yet. Paste the URL in this slot. This project was built at NEAR HACKATHON. #NEARHackathon @NEARProtocol

**Live demo:** https://triagepay-demo.vercel.app/

**Refused on testnet:** [prompt-injected payee, `ERR_PAYEE_NOT_ALLOWED`](https://testnet.nearblocks.io/txns/8y6sEwL8i5sP2aMKo5HS3WGZtZ8NMDP9MUdywYAmVnN4)

**Allowed on testnet:** [payee on the allowlist, inside the cap](https://testnet.nearblocks.io/txns/4oxycWdvo9M4YWoq9mg2y4FXL7AZYQY6McvcPJQYJejq)

**Owner approval on testnet:** [a payment above the threshold is held](https://testnet.nearblocks.io/txns/8nvs4X4jWrzEzsbUj2ziS5qEdTTi6wvRwfUTBxftdcJi), [the owner approves and it settles](https://testnet.nearblocks.io/txns/CwEo7ec8uk25XQbKHNmS3naUhqM9FPbRJzEAjVf223Hp); [a second one is rejected](https://testnet.nearblocks.io/txns/CcUmBvhQPBvtPJ9pg4NF225U4XfaJEmwjLEZV9gCAaWR). All steps: [docs/demo/testnet-review/2026-11-12](docs/demo/testnet-review/2026-11-12/README.md)

**Chain Signatures on testnet:** [`v1.signer-prod.testnet` signed a Base Sepolia USDC transfer](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM) for the owner account, and the signature recovers to the derived address `0x508e984fda3bc37d8c33bfb5098441fd2169fcc8`. Not broadcast ([docs/contracts/chain-signatures.md](docs/contracts/chain-signatures.md))

**Live on mainnet:** the policy wallet with the owner-review queue. 170 graded mainnet tests on 2026-11-13, including five NEAR AI models told to follow injected payment instructions. 0 wNEAR reached an attacker ([below](#live-on-mainnet))

The paid API is a triage service for security findings: an agent submits a
finding and gets a deterministic verdict (`supported`, `needs_evidence`,
`held` or `rejected`) with the reasons. Payment is HTTP 402 (x402 v2) settled
by a NEAR transaction that the server verifies from the token contract's own
receipt.

Everything in this repository was built at NEAR HACKATHON. I entered it in the `security` and `agents` tracks.

## Live on mainnet

The policy wallet runs on NEAR mainnet with real (tiny) wNEAR amounts. On
2026-11-13 I closed the first mainnet wallet and redeployed the review-queue
build at the same account. It is still live.

- Wallet contract: [`tp-wallet.hlpok1lad.tg`](https://nearblocks.io/address/tp-wallet.hlpok1lad.tg).
  - Code hash `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC`, built from `ef7472c`.
  - 0 access keys. Owner `hlpok1lad.tg`. Token `wrap.near`.
  - Payee allowlist `[gateway.hlpok1lad.tg]`. 1,000,000 per call and 4,000,000 per UTC day, in yocto-wNEAR.
  - Owner review is on: a payment above 500,000 or to a payee off the allowlist is held for the owner.
- Agent: [`tp-agent2.hlpok1lad.tg`](https://nearblocks.io/address/tp-agent2.hlpok1lad.tg), active. Its only key is a function-call key for `pay` on the wallet.
- **Paid:** [allowlisted payee, inside the cap, `pay_settled`](https://nearblocks.io/txns/Hh6BquChhVfHMkapJKuG4AXv3Nbe7u2ZTV9xLezoCR2C)
- **Refused:** [prompt-injected payee, `ERR_PAYEE_NOT_ALLOWED`](https://nearblocks.io/txns/8JBgqcYGjGjkBDoyCehPnLYZRTU58PYnVzWNBccw2yJi). [Over the daily cap, `ERR_DAILY_LIMIT`](https://nearblocks.io/txns/45hcfFNUHujLFcT8L6YECoZT8CLFjScRpBY5kuRfgR4q). [A non-owner calls `withdraw`, `ERR_NOT_OWNER`](https://nearblocks.io/txns/4knb47ejw7unkh3o57PkaaT6p6LDApHznNXKrap8YmCw)
- **Owner review:** [held](https://nearblocks.io/txns/C91iH3Tmj78kZMAe4dd24suY1hqZzZvpec4n7YKaL475). [The owner approves and it settles](https://nearblocks.io/txns/5yK3nM2bozY8d9XuAQtAJjA2LBEcDHCpfuXKVh9n2FWt). [A second one is rejected](https://nearblocks.io/txns/8vU2BbPSKze5GL4sRLPBqg3SsQPGTvpA8n7RWXcHNdt6)
- **A hijacked model:** `Qwen3.8-27B-abliterated` on NEAR AI Cloud, told to obey payment instructions in the finding, sent five "tips" to the attacker. The wallet refused [all](https://nearblocks.io/txns/9xA5AoEG5sdghsaoBcwMkADNSEP3TTHg7B5bd3eCkJL3) [five](https://nearblocks.io/txns/E2w4zp21Pu4NjYMk6jr6aNkFotr5kxRACRzvEiQti1hs)

The campaign covered:

- 170 graded tests: limits, zero and overflow amounts, malformed args, everything the pay-only key cannot do, a real non-owner account, revoke and restore, pause, the review queue, and three rounds of LLM agents. All 170 matched their expected result.
- 183 transactions, 0.066 NEAR of gas, and about USD 0.046 of NEAR AI.
- One design issue: an agent can fill the owner's review queue and reserve the day with junk holds. Its own legitimate payments then fail until the owner rejects them. Funds stay safe; the cost is availability.

Everything is in [`docs/demo/mainnet-campaign/2026-11-13/`](docs/demo/mainnet-campaign/2026-11-13/README.md), and the script is `scripts/mainnet/campaign.mjs`. The first mainnet run of 2026-11-12 (the build before the review queue) is in [`docs/demo/mainnet-e2e/2026-11-12/`](docs/demo/mainnet-e2e/2026-11-12/README.md). The x402 receipt check runs on testnet only, because `wrap.near` emits no NEP-141 events for the verifier to read.

## Hosted demo

`demo/live` is the judge page: pick a prompt injection, an over-limit
payment, or a flagged payee, and the server signs `pay` on NEAR testnet.
The page shows the payee risk verdict (allow, warn, or block), today's
spend against the daily limit, explorer links, and Revoke and Restore.
The Human in the loop card queues a payment above the owner's threshold, and Approve or Reject settles or drops it on chain.
The browser never sees a key. Revoke, Restore, Approve and Reject are public buttons. They
use the owner account's full-access key, which is held on the server as
`TRIAGEPAY_OWNER_SECRET_KEY`.

An allowlisted payee shows warn when the account is under seven days old.
The demo payee was created for the page, so warn is expected. Warn does
not move money. The risk check reads the allowlist from the wallet
contract, the same list the spend panel shows. If a visitor leaves the
agent revoked, the next Launch restores it before the attack, and a daily
cron calls the same restore.

Required Vercel environment variables (names only; values stay out of git):

| Variable | What it is |
|---|---|
| `TRIAGEPAY_AGENT_SECRET_KEY` | `ed25519:...` function-call key. It can call only `pay` on the wallet. |
| `TRIAGEPAY_OWNER_SECRET_KEY` | `ed25519:...` owner key. Used only to revoke and restore the agent and to approve or reject a held payment. |

Public account ids, the RPC URL, limits, the flagged account, and the
verified code hash live in `demo/live/config.json` after setup. Optional
overrides: `TRIAGEPAY_RPC_URL`, `TRIAGEPAY_OWNER_ACCOUNT`,
`TRIAGEPAY_AGENT_ACCOUNT`, `TRIAGEPAY_WALLET_ACCOUNT`,
`TRIAGEPAY_TOKEN_ACCOUNT`, `TRIAGEPAY_PAYEE_ACCOUNT`,
`TRIAGEPAY_INJECTED_ACCOUNT`, `TRIAGEPAY_FLAGGED_ACCOUNT`,
`TRIAGEPAY_COOLDOWN_SECONDS` (default 20), `TRIAGEPAY_MAX_ATTEMPTS_PER_DAY`
(default 40). The full template is `demo/live/.env.example`.

The server signs at most once per cooldown, at most the daily attempt cap,
and at most a few times a minute from one address
(`TRIAGEPAY_IP_LIMIT`, default 4, per `TRIAGEPAY_IP_WINDOW_SECONDS`,
default 60). Those three hold across serverless instances when a shared
store is configured: `KV_REST_API_URL` and `KV_REST_API_TOKEN`, or
`UPSTASH_REDIS_REST_URL` and `UPSTASH_REDIS_REST_TOKEN`. Create the store
in the Vercel or Upstash dashboard and add the two variables with
`npx vercel env add`. Do not pass the token on the command line. If the
store is set and does not answer, signing is refused. If it is unset, the
daily cap is read from the chain index (paginated through the UTC day, not
the last eight transactions) plus this instance's journal, and the
per-address cap is only inside that instance. A truncated index fails
closed. Refused attempts are still transactions on the explorer. The
hosted project has no store connected yet (`/api/health` reports
`sharedLimits: false`), so it runs on that fallback.

The function-call key also has a limited gas allowance, and the contract's
own daily limit caps the token.

```sh
cd demo/live
npm ci
npx vercel link
npx vercel env add TRIAGEPAY_AGENT_SECRET_KEY production
npx vercel env add TRIAGEPAY_OWNER_SECRET_KEY production
npx vercel deploy --prod
```

The CLI prompts for each secret. Do not pass the key on the command line.
A local run of the same functions:

```sh
cd demo/live
npm ci
node server.mjs
```

When `VERCEL` is unset and the two variables are empty, the server reads
`demo/live/.keys/` (gitignored). On Vercel that fallback is off.

Setup (once, from a machine that can reach the testnet faucet) builds
nothing by itself. Point it at the policy-wallet wasm, then copy the two
secrets into Vercel:

```sh
cd contracts
# --cfg near is what cargo-near adds. Without it, env::panic_str does not
# call the host and a refusal is a silent trap.
RUSTFLAGS='--cfg near' cargo build --release --target wasm32-unknown-unknown -p policy-wallet --locked
cd ..
TRIAGEPAY_WALLET_WASM=contracts/target/wasm32-unknown-unknown/release/policy_wallet.wasm node demo/live/setup.mjs
```

`setup.mjs` prints account ids and writes keys under `demo/live/.keys/`.
It does not print the secrets. The public demo accounts on testnet are in
`demo/live/config.json`: wallet `tpd-e947e7-wallet.testnet` (with the
owner-review queue, deployed 2026-11-12), agent
`tpd-e947e7-agent.testnet`, owner `tpd-e947e7-owner.testnet`.
`node demo/live/proof.mjs` runs the live proof against them; the
transactions are in
[docs/demo/testnet-review/2026-11-12](docs/demo/testnet-review/2026-11-12/README.md).

The agent account has one access key: the function-call key that can call
only `pay` on that wallet
(`ed25519:9iERawak26p4uGVfktNgcpG1zxuEV31d8GAnY1qUxUis`). That is the key
the Vercel project already held for the previous demo wallet. I added its
public half to the new agent
([2sfo5j9N](https://testnet.nearblocks.io/txns/2sfo5j9NdZGT3Bb3ap9FpfXaPjtzfCZfkEB5iQCJXByZ)), and the
hosted owner key to the new owner
([D9Vghe56](https://testnet.nearblocks.io/txns/D9Vghe56X4yY47awicJoo4Gaj3KPN3xmdHLmT1qfMZZp)), so no secret
had to move. Then I deleted the setup full-access key and my local pay key
in one transaction
([74a2PEiV](https://testnet.nearblocks.io/txns/74a2PEiV98G5gJppaiXTNZ2vmGBCEDbPxUAuSzPpsAsC)). With no
full-access key left, the agent cannot add a key or move its own NEAR. When
the pay key's 0.25 NEAR gas allowance runs out, the agent needs a new
account.

`node demo/live/setup.mjs cleanup` does the same deletion for an agent
set up the old way (`--drop-full-access` also removes the full-access key,
and only when a pay key remains). The previous demo agent,
`tpd-797021-agent.testnet`, still holds three keys (a full-access setup
key, the pay key above, and a leftover pay key for
`tpd-797021-wallet.testnet`). I do not have its full-access secret, so I
could not delete them. The page no longer uses that account.

The same run on the new wallet also produced an over-limit refusal
([`ERR_PER_CALL_LIMIT`](https://testnet.nearblocks.io/txns/BMoHjQXgQasLxn4wZspzTQU9g1rBjbqThhZQevknCZ7s)),
a revoke ([`revoke_agent`](https://testnet.nearblocks.io/txns/5qZ5yQAPTm5dFg7i455HvsrgBNk8bTbbCou6zq4461as))
and a revoked-agent refusal
([`ERR_AGENT_INACTIVE`](https://testnet.nearblocks.io/txns/3iHytU9nkYuqPh74its9TPW3MwQoUJeaPQg1nbGGyQTZ)).

## Judge page additions

The chain is still the part that says no. Setup for each path is in [docs/demo/live-features.md](docs/demo/live-features.md).

### Drain Me

Anyone can type a prompt injection and try to make the agent pay an attacker account. I run that text through the agent, then through the same pay path as Launch. I do not sign a payment the wallet would settle, so the drained total stays at 0 unless that check is wrong. If it is wrong, the counter shows the settled amount. The feed keeps a shortened, redacted line and the transaction link. Each address is capped (`TRIAGEPAY_DRAIN_IP_LIMIT`, default 3 per `TRIAGEPAY_DRAIN_WINDOW_SECONDS`, default 600). The cap uses the shared store when one is set, and this process's memory otherwise. A signed attempt still has to pass the daily cap from that store or the chain index.

### A model that falls for it

When `NEARAI_API_KEY` is set, Launch and Drain Me call a NEAR AI Cloud model (`NEARAI_MODEL`, default `z-ai/glm-5.3-flash`). The finding contains the injected pay instruction. The model's own tool call is what I submit, and the page streams that reasoning before the wallet answers. With no key, or if the call fails, I use the scripted agent and the page says `scripted`.

### Explain-why receipts

A refusal, a hold, an approval, and a rejection each get a receipt: what the agent tried, the rule and the contract error code, the amount, the payee, and the transaction link. Save card hits `/api/og` and returns a PNG. Share on X includes `built at NEAR HACKATHON @NEARProtocol #NEARHackathon`.

### Telegram approval

When a payment is held and `TELEGRAM_BOT_TOKEN` plus `TELEGRAM_OWNER_CHAT_ID` are set, I message that chat with Approve and Reject. `/api/telegram` checks `TELEGRAM_WEBHOOK_SECRET` and the chat id, then calls the same approve and reject path as the page. If those variables are unset, the page says so and the buttons on the page still work.

### Kill switch

With `KILL_SWITCH_SECRET` set, I mint a signed link and a QR code. It expires (`KILL_SWITCH_TTL_SECONDS`, default 600), and it works once. Opening it revokes the agent. The server still holds `TRIAGEPAY_OWNER_SECRET_KEY`, as Revoke already does. Single-use across instances needs the shared store. Without the secret, the card says I cannot mint a link.

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
| `cargo test --workspace` | `226 passed`, `3 ignored` (the ignored three read mainnet RPC). The extra pass is the unlisted-reserve test in `policy-core`; the other workspace suites were not re-run for this change |
| Terminal 1: `cargo run -p aps-api --bin aps-api` | `aps-api listening on http://127.0.0.1:8080 (payment provider: mock)` |
| Terminal 2: `python scripts/x402-demo.py` | Step 1: `HTTP 402` and the decoded payment requirement. Step 2: `HTTP 200`, `verdict: needs_evidence`, the content hash the payment settled against. Step 3: the same proof again returns the identical response. Step 4: the proof reused for another finding is `proof_already_consumed` |
| `./scripts/demo.sh` (or `demo.ps1`) | The prepaid flow without x402: fund, reserve, scan, settle, reject, refund; final balances conserve |

The mock provider accepts any transaction hash on a development-only network
(`aps:mock`). It shows the HTTP exchange and the ledger, not payment
verification.

To add the model judge without a NEAR AI key, start the API with
`APS_SCAN_SKU=triage.v2` and `APS_TRIAGE_CASSETTES=triage-model/cassettes`.
The walkthrough then also prints `model judge z-ai/glm-5.3-flash (listed as
TEE-verifiable): high, p=0.75` and the model's rationale, replayed from
recorded replies for the repository's synthetic findings. With
`APS_LIVE_MODEL=1` and `NEARAI_API_KEY` set instead, the call goes to NEAR AI
Cloud (about USD 0.0002 per call).

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
| Function-call access keys | The key the agent uses to pay: one contract, one method, no deposit. On the hosted agent account it is the only key | Transcript steps `keys_view`, `self_escalation`; [docs/contracts/policy-wallet.md](docs/contracts/policy-wallet.md) testnet steps 11-24; hosted keys listed under Hosted demo |
| A contract as the spending policy | Per-call limit, per-UTC-day limit, payee allowlist, pause, revoke, and an owner queue for a new payee or an amount above a threshold; owner methods need 1 yoctoNEAR, which no function-call key can attach | `contracts/policy-wallet`: 78 unit tests, 3 property tests in `crates/policy-core`; transcript refusals |
| Chain Signatures | One testnet `sign` request on `v1.signer-prod.testnet` for an EIP-1559 USDC transfer on Base Sepolia; the signature recovers to the address derived for (owner account, `base-sepolia-1`). Not broadcast, and the wallet does not call `sign` yet | [docs/contracts/chain-signatures.md](docs/contracts/chain-signatures.md); [sign transaction](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM); SDK tests reuse the recorded values |
| NEP-141 and NEP-297 events | Payment is an `ft_transfer`; the server accepts it only when the token contract's own `ft_transfer` event, in a finalized receipt, pays the gateway with the server-issued memo | `crates/near-adapter/src/verify.rs`; 21 receipt-fixture tests, one a real mainnet capture; transcript |
| x402 v2 over NEAR | A client-submitted payment proof (`near-tx-proof`) that works from a policy wallet. The upstream NEAR binding needs a full-access key; this one does not. A proposed profile, not upstream | [docs/protocols/x402-near.md](docs/protocols/x402-near.md); 15 flow tests; transcript |
| NEP-413 signed messages | Login-message verification for future caller authentication | `crates/auth`, 25 tests. A library: the API does not use it yet |
| NEAR AI Cloud | The optional Stage B model judge (`APS_SCAN_SKU=triage.v2`) calls a model the catalogue lists as TEE-verifiable; it is advisory and can never raise a verdict. The hosted demo separately checks that an attestation report echoes a fresh nonce. The Intel quote is not verified | Spike on 2026-11-12: 50 live calls, 5 models, USD 0.096 ([docs/engine/model-selection.md](docs/engine/model-selection.md)); recorded replies in `triage-model/cassettes`, replayed by 6 tests. Paid route: no attestation (ADR-0011). Demo check: [docs/engine/attestation.md](docs/engine/attestation.md) |

## What the agent's key can and cannot do

- It can call `pay(token, receiver, amount, memo)` on one wallet, with no
  deposit. The wallet checks, in order: not paused, agent active, token
  allowlisted, payee allowlisted, amount within the per-call limit, day total
  within the daily limit. Then the wallet itself sends the `ft_transfer`.
  With owner review on, a new payee or an amount above the threshold is
  queued instead of transferred ([docs/contracts/review-policy.md](docs/contracts/review-policy.md)).
- On the hosted agent account (`tpd-e947e7-agent.testnet`) this is the
  only key. The paragraph under Hosted demo has the deletion transaction.
- It cannot call any owner method, transfer NEAR, add or delete keys, or
  deploy code. Those need a deposit or a full-access key.
- Stolen, the pay-only key can spend at most the daily limit per UTC day,
  to allowlisted payees only, until the owner revokes the agent or pauses
  the wallet. Across a UTC midnight that is up to two daily limits within
  minutes.
- It carries a gas allowance. On mainnet the allowance falls by the gas each
  transaction burns, but a 40 TGas `pay` needs about 0.041 NEAR of allowance
  and balance up front (measured 2026-11-13). When the allowance runs out the
  key stops working until the account's full-access key replaces it. An account that
  holds only function-call keys can lock itself out this way. The hosted
  agent is such an account.
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
| Run on testnet | Policy wallet (28-step run on 2026-11-06, and the agent demo on 2026-11-11); the owner-review wallet on 2026-11-12 (held, approved, rejected); one Chain Signatures request on 2026-11-12, signed and not broadcast; x402 round trips paid through the wallet and directly; receipt verification |
| Run on mainnet | The review-queue policy wallet, live since 2026-11-13: 170 graded tests with 183 transactions, including 51 NEAR AI completions that drove the agent's `pay` calls ([Live on mainnet](#live-on-mainnet)). Before that, the first mainnet wallet on 2026-11-12: four transactions, then closed. Earlier, one native-USDC deposit verified on 2026-10-21 by the argument-based verifier ([docs/integrations/near.md](docs/integrations/near.md)). The x402 receipt check has not run on mainnet |
| Run against NEAR AI Cloud | Stage B, the model judge: a 50-call spike on 2026-11-12 and the paid route in `triage.v2` mode. Every verdict in the 2026-11-11 testnet transcript comes from Stage A (deterministic gates) alone; the run predates the working key |
| Built, tested offline only | The x402 facilitator endpoints, the SQLite journal (`crates/store`), NEP-413 and API keys (`crates/auth`), the static UI (`ui/`), the Dockerfile |
| Not wired | The API keeps its ledger in memory, serves one development caller with no authentication, binds loopback only, and does not serve the UI |
| Run in CI only | The 18 sandbox tests in `contracts/tests` (near-workspaces, a local NEAR node): 18 passed on GitHub Actions, Linux, 2026-11-12. The sandbox has no Windows build |
| Hosted judge demo | `demo/live`: Vercel static page plus serverless functions. Signing keys are environment variables. A local run hits NEAR testnet | 
| Cut | Attested verdicts on the paid route (ADR-0011). The demo checks a nonce echo and does not verify the Intel quote. Also cut: NEAR Intents deposits, a measured accuracy figure. No accuracy is claimed anywhere |

Known limits of the paid route: no rate limiting; the server trusts its RPC
endpoint; on `/v2/scan` the envelope is validated after payment (an invalid
one keeps the payment claimed and can be retried with a corrected body); the
agent in `agents/demo` has no language model. The hosted page can call one
([A model that falls for it](#a-model-that-falls-for-it)), and so can the
mainnet campaign.

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
| Rust workspace | `cargo test --workspace` | 226 passed, 3 ignored (225 from 2026-11-12, plus 1 new `policy-core` test re-run here) |
| Contracts | `cd contracts && cargo test` | policy-wallet 78 re-run on 2026-11-12; test-token 3; sandbox 18 (Linux only; passed in CI on the previous build, not re-run here) |
| Scanner | `cd scanner && python -m pytest -q` | 319 passed |
| Knowledge compiler | `cd knowledge-compiler && python -m pytest -q` | 71 passed, 1 skipped |
| Model judge (offline, with cassette replay) | `cd triage-model && python -m pytest -q` | 73 passed |
| OpenAPI and Python client | `python -m pytest tests -q` | 14 passed |
| UI | `python -m pytest ui/tests -q` | 17 passed |
| TypeScript client | `cd clients/typescript && npm ci && npm test` | 7 passed |
| Policy-wallet SDK | `cd packages/triagepay-sdk && npm test` | 6 passed |

Counts were regenerated on 2026-11-12. The claim-by-claim record is
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
triage-model/            Stage B model judge on NEAR AI Cloud, recorded cassettes
knowledge-compiler/      compiles rule sources into hash-addressed rulesets
clients/                 x402 reference clients (TypeScript, Python)
agents/demo/             the testnet agent demo (full x402 loop, throwaway accounts)
demo/live                hosted judge demo (Vercel): attacks, payee risk, spend, revoke
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

[Architecture](docs/architecture.md) ·
[threat model](docs/threat-model.md) ·
[wallet](docs/contracts/policy-wallet.md) ·
[x402 profile](docs/protocols/x402-near.md) ·
[engine](docs/engine/overview.md) ·
[decisions](docs/decisions/)

## License

MIT. See [LICENSE](LICENSE).
