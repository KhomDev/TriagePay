# Threat Model

> Controls marked here were re-checked by the red-team review of 2026-11-13; the rows it changed
> (state exhaustion, log leakage, prompt injection, mainnet key handling) are updated below, and
> every finding is in [security/redteam-2026-11-13.md](security/redteam-2026-11-13.md).


Assets: the tokens in an agent's policy wallet, prepaid credit, payment
receipts, and the privacy of the compiled knowledge behind the scanner.

This table covers the service. The wallet's own threat table is in
docs/contracts/policy-wallet.md and the payment-proof threats are in
docs/protocols/x402-near.md.

## Abuse cases and controls

| # | Threat | Vector | Control | Verified by |
|---|--------|--------|---------|-------------|
| 1 | Double spend | replay the same deposit request | caller+key idempotency; replay returns the stored receipt | ledger replay tests |
| 2 | Key collision attack | reuse a key with a changed payload to alter a recorded result | payload mismatch is a hard conflict error | conflict tests |
| 3 | Reservation race | concurrent reserves spend the same credit | single-writer ledger behind RwLock; sufficiency checked atomically with the debit | API concurrency tests (crates/api/tests/concurrency.rs: reservation race, double-poll funding, concurrent scans) |
| 4 | Free work | settle without a report | settlement requires Running state and a non-zero 32-byte hash | settle validation tests |
| 5 | Refund theft | refund a settled job, or refund twice | Settled is terminal; Refunded is terminal; replay returns the stored receipt | refund transition tests |
| 6 | Prompt injection through a finding | instructions embedded in title, markdown, code comments, JSON | flag-only detection over the text and an NFKC-folded view without format characters or emphasis marks; gates are deterministic and content-addressed; the combiner never raises Stage A's decision. The flag is a heuristic list, and an obedient model can still move the report's `confidence_band` and `score` (not the decision) | injection corpus tests; `test_flag_sees_through_emphasis_fullwidth_and_format_characters` |
| 7 | Taxonomy extraction | probe the scanner to reconstruct private rule structure | gate outcomes are public, coarse, and content-hashed; no private labels, paths, or identifiers in serialized output | privacy scan tests |
| 8 | Payment spoofing | client asserts "paid" | credit is funded only from provider-observed confirmations; client assertions are never sufficient | payment_flow, near-adapter provider_rpc (memo/recipient/contract/deposit/amount mismatch matrix), and near_flow route tests |
| 9 | Closing someone else's payment requirement | a third party hints an unrelated failed transaction, or a one-unit transfer carrying the memo, at a pending requirement | only an exact-amount confirmation is final; a failed transaction is reported for that hint only and a wrong amount becomes an orphan; memos end in a 128-bit random nonce | x402_flow (`an_unrelated_failed_transaction_does_not_block_the_real_payment`, `a_dust_transfer_with_the_memo_does_not_block_the_exact_payment`, `requirement_memos_carry_an_unguessable_nonce`); live check in docs/demo/transcript-2026-11-11.md |
| 10 | Account enumeration | harvest account ids via error differences | uniform 404 semantics for unknown accounts and jobs | API tests |
| 11 | State exhaustion | flood unpaid `/v2/scan` requests, present bogus transaction hashes, or reserve cheaply | expired requirements without chain evidence of a payment are dropped one hour after expiry, whether or not a hint was presented; the JSON-RPC read runs outside the provider lock; reservations are priced from the price book; one proof starts at most three scans | x402_flow (`expired_requirements_nobody_paid_are_forgotten`); `a_bare_hint_keeps_an_intent_only_for_the_grace_window`; `near_reservations_are_priced_by_the_server`; concurrency.rs. No rate limit exists: see residual risks |
| 12 | Log leakage | finding text or secrets in logs | no request-body logging; redacted structured events; 400 bodies name the kind of JSON failure, never a field name | error-hygiene canary suite (crates/api/tests/error_hygiene.rs, including `unknown_field_names_are_not_reflected_by_typed_routes` for the serde-backed routes) |
| 13 | Oversized payloads | memory pressure via huge fields | strict per-field length limits, evidence count caps | schema tests |
| 14 | SSRF via evidence URLs | scanner fetches attacker URLs | the engine never fetches URLs; evidence is opaque data | design; no fetch code exists |
| 15 | Credit without payment | call the direct deposit route, or settle a job against a made-up hash | both routes answer 403 unless the mock provider is active | x402_flow (`dev_only_routes_refuse_under_the_near_provider`); live check in the transcript |
| 16 | Agent key theft or a prompt-injected agent | the agent's key is used to pay an attacker or to overspend | the key is a function-call key for `pay` on the policy wallet; the contract enforces the payee allowlist and the per-call and daily limits; the owner can revoke or pause | wallet unit tests; docs/demo/transcript-2026-11-11.md (`injected_payment`, `over_per_call`, `scan:over-daily-limit`, `self_escalation`, `pay_after_revoke`) |
| 17 | Scanner internals in responses | scanner stderr or launch errors reach a 500 body | the body names the kind of failure only; one line with the exit code goes to the server's stderr | scan.rs `bridge_failure_message` |

## Residual risks (accepted for the hackathon build)

- In-memory ledger: process restart loses state. Acceptable for the
  demonstrator; a durable store is the first post-hackathon milestone.
- Single dev caller: no per-caller authentication yet. Documented
  limitation of the milestone, tracked for the auth milestone.
- No rate limiting at the HTTP layer. Unpaid requirements are pruned after
  they expire, so a flood holds memory for one validity window, but nothing
  slows the flood, and each proof presentation costs one RPC read.
- Payment state (issued requirements, consumed proofs, orphan records)
  lives in the process with the ledger and is lost with it.
- The NEAR adapter verifies deposits on demand (caller-supplied transaction
  hash hints) rather than via a listener, and trusts its configured RPC
  endpoint; both are documented in docs/integrations/near.md with the
  listener as the post-hackathon upgrade.

## Out of scope by design

- Mainnet key handling: the repository contains no keys. The scripts in
  `scripts/mainnet/` do sign mainnet transactions, from key files outside
  the repository that the operator names on the command line; the hosted
  demo signs mainnet only with a pay-only function-call key when
  `TRIAGEPAY_MAINNET_AGENT_SECRET_KEY` is set.
  Mainnet is touched only by three opt-in read-only tests and by the one
  operator-approved deposit of 2026-10-21 (docs/integrations/near.md).
- Arbitrary URL fetching: the scanner never dereferences evidence URLs.
- Real-world target analysis: all fixtures are synthetic.
