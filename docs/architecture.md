# Architecture

TriagePay has three parts: a wallet contract that limits what an agent's key
can spend, an HTTP service that sells one call at a time behind HTTP 402 and
verifies each payment on chain, and a deterministic engine that produces the
verdict the call pays for.

## Components

```
 agent process                         NEAR
 (function-call key: pay only)
        |                         +--------------------+
        | 1. POST /v2/scan        |  policy wallet     |  per-call limit,
        |    <- 402 + requirement |  (no access keys)  |  daily limit,
        |                         +---------+----------+  payee allowlist
        | 2. pay(token, payee,              | ft_transfer, 1 yocto
        |        amount, memo) ------------>|
        |                         +---------v----------+
        |                         |  NEP-141 token     |  logs the nep141
        |                         +--------------------+  ft_transfer event
        | 3. POST /v2/scan + proof
        v
 +----------------------+   tx_status, FINAL    +-----------------+
 | aps-api (Rust, axum) |---------------------->| NEAR JSON-RPC   |
 |  x402 transport      |<----------------------| (receipt tree)  |
 |  prepaid routes      |                       +-----------------+
 +----+-----------+-----+
      |           | subprocess, stdin/stdout
      |           v
      |     +---------------------+     +------------------------+
      |     | aps-scanner (Python)|- - >| triage-model (Stage B) |
      |     | validate, normalize,|     | optional, advisory,    |
      |     | Stage A gates,      |     | never run live         |
      |     | report + hash       |     +------------------------+
      v     +---------------------+
 +----------------------+
 | aps-core ledger      |  integer money, reserve -> settle against the
 | (in memory)          |  report hash, or refund
 +----------------------+
```

| Component | Location | Role |
|---|---|---|
| Policy wallet | `contracts/policy-wallet`, `crates/policy-core` | Holds one NEP-141 token for its owner; `pay` enforces the agent's limits and sends the transfer |
| Payment provider | `crates/payment`, `crates/near-adapter` | Deposit intents, pure reconciliation, receipt-level verification over JSON-RPC; signs nothing |
| API | `crates/api` | x402 transport (`/v2/scan`), facilitator endpoints, prepaid routes, scanner bridge |
| Ledger | `crates/core` | Dependency-free prepaid ledger with idempotent deposits, reservations, settlements and refunds |
| Engine | `scanner/`, `knowledge-compiler/` | Envelope validation, hostile-input normalization, Stage A gates from a compiled ruleset, content-hashed report |
| Model judge | `triage-model/` | Stage B: prompt, schema, retry, cache. Offline tests only |
| Not wired | `crates/store`, `crates/auth`, `ui/` | SQLite journal, NEP-413 and API keys, static pages |

## Language split

- **Rust** owns everything that moves or accounts for money: the contract,
  the ledger, verification, the HTTP surface. `aps-core`, `aps-payment` and
  `policy-core` have no dependencies; adding one needs an ADR.
- **Python** owns finding analysis. It runs as a subprocess, uses the
  standard library only, never touches money and never sees an account.

## One paid call

1. The server issues a requirement: asset, payee, exact amount, and a memo
   (the deposit-intent id, ending in a 128-bit random nonce). It expires
   after 900 seconds by default.
2. The agent pays with that memo: directly with `ft_transfer`, or through
   the wallet's `pay`.
3. The agent presents the transaction hash. The server reads the finalized
   receipt tree and accepts only the asset contract's own transfer event to
   the payee with the memo (`crates/near-adapter/src/verify.rs`).
4. An exact amount funds the intent once: a ledger deposit under the
   idempotency key `intent:<id>`. Anything else never becomes credit.
5. The ledger reserves the price, the scanner runs, and the job settles
   against the report's content hash. A failed scan refunds the job and
   keeps the credit for a retry with the same proof.
6. The proof is single-use: the same proof and request replay the stored
   response; a different request is refused.

## Job state machine

```
Reserved --> Running --> Settled   (terminal)
   |            |
   +--> Refunded <--+             (terminal)
```

Illegal edges are typed errors. Settling twice with the same hash is a
replay; with a different hash it is an error.

## Trust boundaries

- **Caller to API.** All input is untrusted; bodies are strict-schema. One
  development caller acts for every request: there is no authentication yet.
- **Hint to chain.** A transaction hash only locates a transaction. Every
  fact is re-read from the chain, and a hint anyone can submit never closes
  a requirement someone else is paying.
- **API to scanner.** Finding text is hostile data. Embedded instructions
  are flagged and never change an outcome.
- **API to RPC.** The configured endpoint is trusted; a hostile one could
  fabricate a confirmation.
- **Agent to wallet.** The agent's key reaches only `pay`. Limits hold
  whatever the agent decides.
- **Public repository to private rules.** The engine executes compiled
  rulesets; only a public one is committed.

## Invariants

1. Conservation: `deposited_total == available + reserved + settled_total`
   for every account, checked after every operation by a randomized test.
2. Integer money only.
3. Reservation before work; settlement only against a non-zero report hash;
   a refund returns credit once.
4. Replay never moves funds twice.
5. The wallet never lets an agent's day total exceed its daily limit
   (property test `spend_never_exceeds_daily_max`).

## Deployment shape today

A single process on loopback with the ledger and the x402 state in memory:
a restart loses both. Persistence, authentication, rate limits and a hosted
deployment are built as separate crates and configuration and are not wired
in (ADR-0012).
