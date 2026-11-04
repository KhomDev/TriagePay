# Claims Audit

Every testable claim in the public documentation, mapped to its evidence.
This file exists so a reviewer (human or AI judge) can verify the repo
against its own promises without trusting prose. Audit date: 2026-10-21,
taken at commit `3139b75` (the commit that records this refresh changes
documentation only). Previous audit: 2026-10-20.
**Rule: if a claim is not green here, it is a defect - fix the doc or the
code, never the audit.**

Suites verified for this audit (regenerated 2026-10-21, not hand-edited):
aps-core 21, aps-payment 9, aps-api 37, aps-near 25 (+3 opt-in live
read-only, ignored by default), scanner 69, knowledge-compiler 37 (+1
local-only private-denylist tree scan, skipped unless APS_PRIVATE_DENYLIST
is set), root OpenAPI conformance 4. Total 202 passing. Also green: cargo
fmt --check, clippy -D warnings, ruff, the leakage tree scan
(`python -m knowledge_compiler scan-tree --root ..`: 0 hits against the
published stand-in list in CI, and 0 hits locally with
APS_PRIVATE_DENYLIST set to the operator's off-repo real-term list), and the
voice check (`python scripts/voice-check.py`: 0 violations).

## README.md

| Claim | Evidence | Status |
|---|---|---|
| cargo test --workspace runs the Rust suites | gate run 2026-10-21: exit 0 | GREEN |
| Rust test count | 92 tests across 4 crates (21 core, 9 payment, 37 api, 25 near-adapter) + 3 opt-in live tests | GREEN |
| Python scanner tests | cd scanner && python -m pytest -q: 69 passed | GREEN |
| Knowledge-compiler tests | cd knowledge-compiler && python -m pytest -q: 37 passed, 1 skipped (local-only) | GREEN |
| OpenAPI conformance | python -m pytest tests -q (root): 4 passed | GREEN |
| No keys or external services needed for the default run | mock provider default; secret scan of the tree: 0 hits | GREEN |
| Scanner is stdlib-only Python | scanner/pyproject.toml dependencies = [] | GREEN |
| Core and payment crates have zero external dependencies | their [dependencies] blocks are empty (comment-enforced) | GREEN |
| Demo walks fund through refund with conservation | scripts/demo.ps1 run 2026-10-20: exit 0, final balances conserve | GREEN |
| Integer money only, no floats near a balance | money.rs Amount(u128) with checked ops | GREEN |
| Settlement requires non-zero report hash; refunds one-shot | ledger_ops + error_hygiene tests | GREEN |
| Payment covers the scan, not the verdict | scan_flow settle tests (rejected verdicts settle identically) | GREEN |
| Credit funds only from provider-observed confirmations | payment_flow + provider_rpc + near_flow suites | GREEN |
| Privacy: no protocol names/paths/corpus content in output | test_engine_golden privacy test (digest denylist + host-path prefixes, with a non-vacuity control) + error_hygiene canaries | GREEN |
| Private terms are never published, not even in the privacy tests | real terms are checked off-repo only (APS_PRIVATE_DENYLIST; CI runs without it); the published denylist holds only the digest of a synthetic stand-in (test_published_denylist_holds_only_synthetic_stand_ins); test_guess_list_recovers_nothing_from_the_published_denylist: 0 recovered; local scan-tree with the private list on 2026-10-21: 0 hits | GREEN (earlier commits up to and including 33b8b2f still contain the literal terms or reversible digests of them; history scrub is an open human decision, ADR-0006) |
| Published real-term digests hide the terms (33b8b2f to 01d462e) | a published SHA-256 digest of a guessable word is reversible: a 14-word guess list recovered 10 of the 14 published digests | CORRECTED 2026-10-21 (every real-term digest moved to the operator's off-repo file; the published list keeps a synthetic stand-in only) |
| NEAR adapter targets NEAR Intents 1Click (track fit section) | documented as the v2 roadmap in docs/integrations/near.md; v1 (direct NEP-141 verification) is implemented | GREEN (phrasing kept as target/milestone) |
| Mock provider is the default; NEAR activates only via env | AppState construction + APS_PROVIDER gate in main.rs | GREEN |
| Live mainnet read-only verification | aps-near live_readonly vs https://rpc.mainnet.near.org, run 2026-10-21: 3/3 pass, including native USDC ft_metadata = symbol "USDC", 6 decimals, and legacy USDC.e = symbol "USDC.e", 6 decimals | GREEN (mainnet deposit E2E executed 2026-10-21, see the near.md section) |
| Script and test defaults name the settlement asset | scripts/near-preflight.py --asset fallback USDC_NATIVE_MAINNET; scripts/near-wait-for-deposit.py lists native USDC first; live_readonly's primary candidate is native USDC | CORRECTED 2026-10-21 (all three defaulted to or led with legacy USDC.e) |
| README status: one mainnet deposit E2E executed with real USDC | docs/integrations/near.md "Mainnet end-to-end run" (tx hashes) | GREEN (README sentence updated 2026-10-21; it previously said the run was operator-gated) |

## SCANNER_SPEC.md / scanner/README.md

| Claim | Evidence | Status |
|---|---|---|
| Nine deterministic gates, decision matrix, confidence bands | test_gates branch coverage + test_engine_golden decisions | GREEN |
| Gates are ruleset-driven, fail closed on unknown evaluator or tampered hash | test_ruleset unknown-evaluator, tampered-hash, missing-file | GREEN |
| Report exposes exactly the spec fields (including ruleset_hash) | test_engine_golden field-set test | GREEN |
| content_hash is the sha256 of the canonical normalized envelope | test_engine_golden replay test (identical hashes) | GREEN |
| The committed ruleset artifact is byte-stable | compile --check passes on Linux CI and on Windows (2026-10-21); test_written_artifact_is_lf_and_matches_the_committed_bytes; test_compiler determinism | GREEN (the Windows-only false mismatch - write_artifact wrote CRLF in text mode - was fixed 2026-10-21 by pinning newline="\n") |

## docs/architecture.md, threat-model.md, ADRs

| Claim | Evidence | Status |
|---|---|---|
| Conservation invariant is property-checked | ledger_fuzz (2 seeds x 2400 ops, is_conserved after every op) | GREEN |
| Threat-model row 3 "API concurrency tests" | concurrency.rs (4 tests) | GREEN (was false before this session; made true) |
| Threat-model row 12 verification | error_hygiene.rs (7 tests; found and closed a real reflection bug) | GREEN (was false before; made true) |
| ADR-0001/0002/0003 (integer money, charge timing, idempotency) | ledger_ops + api.rs suites | GREEN |
| ADR-0004 adapter dependency policy | aps-near carries only ureq/serde_json/base64; core/payment dep-free | GREEN |
| ADR-0005 verification-only NEAR adapter | no signing/broadcast code path exists; refunds are proposals | GREEN |
| ADR-0006 window strategy and ADR-0007 product direction | decision records only; every pillar and the README prior-work section / boundary tag are labelled planned | GREEN (organizer answer pending) |
| CI enforces neutral or singular voice in tracked Markdown | .github/workflows/ci.yml "Documentation voice" job; scripts/voice-check.py --self-test 15/15 | GREEN |

## docs/integrations/near.md

| Claim | Evidence | Status |
|---|---|---|
| Every facts-table row | verified live or against official docs on 2026-10-20 (cited per row) | GREEN |
| 1RPC caveat | probed 2026-10-20: NEAR JSON-RPC methods answered -3261 | GREEN |
| USDC.e metadata (6 decimals, Bridged USDC), listed as a legacy token | on-chain ft_metadata read at finality (2026-10-20, re-read 2026-10-21) | GREEN |
| Settlement asset is native USDC (17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1: name "USDC", symbol USDC, 6 decimals) | on-chain ft_metadata read 2026-10-21; asset of the mainnet run (tx ZonAdEjgAUM4tBnCR3nJTxe9LHiv7KcsbqRZdp9dns1) | CORRECTED 2026-10-21 (the near.md facts table, the near.md price-book paragraph, and the .env.example value named bridged USDC.e as the settlement asset, the paragraph said no native USDC existed on NEAR mainnet, and the README status cited only USDC.e metadata; USDC.e is now listed as a legacy token, not used) |
| Adapter never signs or broadcasts | no such code path; ADR-0005 | GREEN |
| Mainnet deposit E2E | executed 2026-10-21 with real USDC: payment tx ZonAdEjgAUM4tBnCR3nJTxe9LHiv7KcsbqRZdp9dns1 verified by the adapter, ledger funded, scan settled (evidence in docs/integrations/near.md) | GREEN |
| Live-network E2E (testnet) | executed 2026-10-20: intent -> memo-bound ft_transfer -> adapter-verified funding -> scan -> settle, all through the real adapter; evidence in docs/integrations/near.md | GREEN |
| Security hardening via a function-call key restricted to ft_transfer (near.md "Security-track angle", PROPOSALS.md section 3, section 6 custody row) | infeasible: function-call keys cannot attach a deposit (https://docs.near.org/protocol/accounts-contracts/access-keys, accessed 2026-10-21) and NEP-141 ft_transfer requires exactly 1 yoctoNEAR; replaced in all three places by the planned spend-policy wallet contract (zero-deposit `pay` callable by a function-call key; the contract performs ft_transfer) | CORRECTED 2026-10-21 (claim withdrawn; replacement labelled planned, prompt 02) |

## SECURITY.md / CHANGELOG.md

| Claim | Evidence | Status |
|---|---|---|
| Request bodies are never logged | no request-body logging in handlers; one startup log line | GREEN |
| No keys and no signing or broadcast path in the codebase | secret scan 0 hits; adapter is verification-only (ADR-0005) | GREEN |
| SECURITY.md posture: real funds moved only in the operator-approved mainnet deposit run | docs/integrations/near.md mainnet section | CORRECTED 2026-10-21 (SECURITY.md previously said no real funds or mainnet endpoints were involved anywhere, which the mainnet E2E made false) |
| SECURITY.md "Non-goals and guarantees" (renamed from a plural-voice heading) | same four guarantees; voice check 0 violations | GREEN |

## Known open items (not doc claims)

- ~~Mainnet E2E~~ DONE 2026-10-21 (real USDC, tx evidence in docs/integrations/near.md).
- ~~Testnet E2E~~ DONE 2026-10-20 (throwaway accounts, evidence recorded).
- The repository has no remote yet; publication is a human action.
- Organizer answer on pre-window work: pending (ADR-0006 default applies).
- Spend-policy wallet contract: planned (prompt 02), not implemented.
