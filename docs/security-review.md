# Security Review

> The latest review is the red-team review of 2026-11-13. Its findings, fixes and regression
> tests are in [security/redteam-2026-11-13.md](security/redteam-2026-11-13.md). The tables below
> are the earlier internal reviews.


Adversarial QA record: threats from docs/threat-model.md mapped to their
control, the test that verifies it, the residual risk, and the owner. The
table below was written on 2026-10-21 for the prepaid routes; the
2026-11-11 review of the x402 path and the wallet is the section after it.

**Suites (regenerated 2026-11-12):** Rust workspace 225 passed and 3
ignored live tests; contracts 65 and 3; scanner 319; knowledge-compiler 71
and 1 skipped; triage-model 73; root 14; ui 17. Per-crate counts are in
docs/claims-audit.md.

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

## Review of 2026-11-11 (x402 path and wallet)

An independent review found these defects; each was reproduced with a
failing test before the fix and is covered by a regression test now.

| Finding | Fix | Test |
|---|---|---|
| Any finalized failed transaction, hinted at a pending requirement, became its final observation; the payer's real payment was then refused | A failed transaction is reported for that hint only | `an_unrelated_failed_transaction_does_not_block_the_real_payment` |
| A one-unit transfer carrying the memo became the final observation | A wrong amount is an orphan and leaves the requirement open | `a_dust_transfer_with_the_memo_does_not_block_the_exact_payment` |
| Memos were a process id, a clock and a counter | 128-bit nonce from the operating system | `requirement_memos_carry_an_unguessable_nonce` |
| `POST /accounts/{id}/deposits` credited any account with the NEAR provider active | 403 unless the mock provider is active; same for `POST /jobs/{id}/settlements` | `dev_only_routes_refuse_under_the_near_provider` |
| Wallet limits were shared by every allowlisted token, whatever its decimals | One token per wallet | `limits_sized_for_one_token_cannot_move_another` |
| A proof presented after expiry left no record; orphans were not readable | The late transaction is read and recorded; `GET /payment/intents/{id}` lists orphans | `an_expired_requirement_never_funds` |
| Unpaid requirements and per-intent locks grew without bound | Expired requirements without chain evidence are pruned one hour after expiry, hinted or not (a bare hint used to keep them forever, red-team 2026-11-13); locks only for issued requirements | `expired_requirements_nobody_paid_are_forgotten`, `a_bare_hint_keeps_an_intent_only_for_the_grace_window` |
| Scanner stderr reached 500 bodies | Fixed message; detail withheld | `bridge_failure_message` |

Not fixed, stated as limits: no rate limiting; one development caller; state
in memory; the envelope on `/v2/scan` is validated after payment (a
validate-first step would spawn a scanner process for every unauthenticated
request, which is the worse trade without rate limiting); one batch
transaction paying two requirements funds only the first presented.

## Findings fixed during the 2026-10-21 review

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
- The adapter trusts its configured RPC endpoint; FASTNEAR's free endpoints
  are the default (the official ones are deprecated) and the trust is
  documented.

## Supply-chain posture

- aps-core and aps-payment: zero dependencies (enforced by policy).
- aps-api: axum, tokio, serde, serde_json (+ tower/http-body-util/base64
  as dev-deps).
- aps-near: ureq (rustls), serde_json, base64 - the minimum for
  JSON-RPC verification - and getrandom for the intent-id nonce.
- Python: stdlib-only at runtime (ruff/pytest are dev tools).
- Recommended next step: cargo-audit/cargo-deny in CI plus a signed
  release process before any real funds.
