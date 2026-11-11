# Security Review

Adversarial QA record for the current milestone: every threat from
docs/threat-model.md mapped to its control, the test that verifies it,
the residual risk, and the owner. Generated from the real test inventory
(all counts regenerated on 2026-10-21).

**Suites:** aps-core 21, aps-payment 9, aps-api 37, aps-near 25 (+3
opt-in live read-only tests), scanner 69, knowledge-compiler 37 (+1
local-only private-denylist check), root OpenAPI conformance 4. Total 202
passing, plus the 3 live tests run explicitly against mainnet.

## Threat → control → verification

| # | Threat | Control | Verified by | Residual risk | Owner |
|---|---|---|---|---|---|
| 1 | Double spend (deposit replay) | caller+key idempotency; replay returns the stored receipt | api.rs deposit_replay_returns_200_without_double_credit; payment_flow confirmation_funds_the_account_exactly_once; near_flow step 6 | in-memory ledger loses receipts on restart (documented limitation) | operator |
| 2 | Key collision (same key, changed payload) | payload mismatch is a hard conflict | api.rs deposit_same_key_different_amount_conflicts; ledger_fuzz same-key-changed-payload sequences | none known | - |
| 3 | Reservation race (concurrent reserves) | single-writer ledger; sufficiency checked atomically with the debit | concurrency.rs reservation_race (10 concurrent reserves on 5 units: exactly 5 succeed, conservation holds) | none known | - |
| 4 | Free work (settle without a report) | settlement requires Running state + non-zero 32-byte hash | ledger_ops settle_requires_running_state_and_nonzero_hash; error_hygiene zero-hash rejection | none known | - |
| 5 | Refund theft (refund settled job / twice) | Settled and Refunded are terminal; replays return the stored receipt | ledger_ops refund_returns_credit_exactly_once, settled_job_cannot_be_refunded; mock_provider refunds_are_one_shot | none known | - |
| 6 | Prompt injection through a finding | flag-only detection; gates are deterministic; outcomes content-hashed | test_normalize_injection corpus (injected vs stripped envelopes produce identical decisions); engine golden replays | injection flag is advisory only by design | - |
| 7 | Taxonomy extraction via probing | gate outcomes are public, coarse, content-hashed; no private labels in serialized output | test_engine_golden privacy test (no protocol names, host paths, private markers in output) | gate names are public by design (spec) | - |
| 8 | Payment spoofing (client asserts paid) | credit funds only from provider-observed confirmations; NEAR adapter re-verifies every fact on chain (status, contract, method, args, memo, deposit, amount) | mock_provider reconciliation_funds_only_exact_matches; provider_rpc (memo/recipient/contract/yocto/amount mismatch tests); near_flow routes | adapter trusts the configured RPC endpoint (a hostile endpoint could fabricate confirmations; mitigated by using official endpoints and documenting the trust) | operator |
| 9 | Quote manipulation | not applicable to the direct NEP-141 flow (no quotes); near refund is proposal-only | provider_rpc refund proposal test (reason marked PROPOSAL ONLY, one-shot) | none in scope | - |
| 10 | Account enumeration | uniform 404 semantics | api.rs unknown_account_and_job_are_404; near_flow amount-mismatch account read | none known | - |
| 11 | Queue exhaustion | reservation sufficiency + refunds on failure; per-account quotas documented as a milestone | concurrency.rs (overdraft impossible); scan timeout refunds documented | no HTTP rate limiting yet (documented limitation) | operator |
| 12 | Log leakage / input reflection | no request-body logging; scanner errors never echo attacker strings; API bounds the joined message | error_hygiene canary suite (found a real reflection bug: unknown field names echoed in 400s - fixed in scanner loader + scan.rs truncation; test un-ignored and green) | stderr scanner-failure text may contain scanner internals (not envelope content) | operator |
| 13 | Oversized payloads | strict per-field length limits, evidence count caps | test_schema boundaries; error_hygiene over-length rejection | none known | - |
| 14 | SSRF via evidence URLs | the engine never fetches URLs | design; no fetch code exists (grep-verified) | none | - |
| 15 | Idempotency across concurrent scans | ledger rejects the second settlement as illegal transition | concurrency.rs concurrent_scan_single_settlement (exactly one charge) | none known | - |
| 16 | Ruleset tampering | payload hash must match the compiled gates; unknown evaluator fails closed | test_ruleset tampered-hash + unknown-evaluator + missing-file tests | artifact is committed, not signed (roadmap: signed releases) | operator |
| 17 | Private knowledge leakage via compiled rulesets | compiler leakage scanner refuses to emit artifacts carrying denylisted terms, host paths, non-allowlisted URLs, account ids, secrets. Real private terms are checked off-repo only: their digests live in the operator's file named by APS_PRIVATE_DENYLIST, never in the repository, because a published digest of a guessable word is reversible by dictionary hashing. The published denylist holds synthetic stand-ins only | test_leakage poisoned-source matrix (8 cases with synthetic stand-ins, all abort), published-list allowlist test, guess-list test (0 recovered), canary compile test; scan-tree with the private list run locally | CI cannot check real terms (by design); the allowlist is small and explicit; anyone holding the private file can recover guessable terms | operator |

## Findings fixed during this review

- **Input reflection (fixed):** the scanner CLI echoed unknown envelope
  field names into error strings, and the API forwarded them verbatim in
  400 bodies - attacker-chosen text reflected into responses. Found by
  the error-hygiene canary suite; fixed in the scanner loader (reports
  the count, never the names) and hardened in scan.rs (per-message and
  total truncation). The canary test now runs un-ignored and green.

## Residual risks (accepted, documented)

- In-memory ledger: a restart loses state (first post-hackathon milestone).
- Single dev caller: no real authentication boundary yet (auth milestone).
- No HTTP rate limiting (bounded by test usage; quota milestone).
- The NEAR adapter verifies on demand (caller-supplied tx hash) rather
  than via a listener; a listener is the post-hackathon upgrade.
- The adapter trusts its configured RPC endpoint; official public
  endpoints are the default and the trust is documented.

## Supply-chain posture

- aps-core and aps-payment: zero dependencies (enforced by policy).
- aps-api: axum, tokio, serde, serde_json (+ tower/http-body-util/base64
  as dev-deps).
- aps-near: ureq (rustls), serde_json, base64 - the minimum for
  JSON-RPC verification - and getrandom for the intent-id nonce.
- Python: stdlib-only at runtime (ruff/pytest are dev tools).
- Recommended next step: cargo-audit/cargo-deny in CI plus a signed
  release process before any real funds.
