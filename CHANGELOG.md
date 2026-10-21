# Changelog

All notable changes to this project are documented here. The format
follows Keep a Changelog; the project has not shipped a tagged release.

## [Unreleased]

### Added
- Prepaid ledger core: integer money, idempotent deposits, reservations,
  settlements, refunds, conservation fuzz, SKU price book.
- Payment provider trait with pure reconciliation and a deterministic
  mock provider (finality delay, mismatched payments, expiry, one-shot
  refunds).
- HTTP API: accounts, deposits, reservations, job lifecycle, scan
  execution, payment intents, single error shape, strict bodies.
- Python scan engine: envelope schema, hostile-input normalization with
  injection flagging, nine deterministic gates, decision matrix,
  content-hashed public reports, CLI bridge.
- End-to-end scan flow over the real subprocess: payment covers the scan,
  not the verdict; invalid envelopes never charge.
- Demo scripts (bash + PowerShell) walking funding through refund.
- Architecture, threat model, ADRs 0001-0003, CI pipeline, security and
  contribution guides.

### Security
- Credit funds only from provider-observed confirmations matching amount
  and asset; client assertions have no funding path.
- Settlement requires the Running state and a non-zero 32-byte report
  hash; refunds are one-shot; replays never move funds twice.

### Added (2026-10-20 session)
- NEAR payment adapter (aps-near): memo-bound NEP-141 deposit intents
  verified on chain over JSON-RPC with wait_until FINAL; no signing or
  broadcasting; refunds as operator-approved proposals; live read-only
  mainnet verification (status, accounts, USDC.e metadata).
- Provider selection behind one enum: mock (default) or near via
  APS_PROVIDER, with fail-fast environment validation.
- Private-knowledge compiler (knowledge-compiler/): deterministic,
  leakage-scanned, hash-addressed ruleset artifacts with a --check CI
  gate and a compatibility checker.
- Ruleset-driven scanner: gates execute the compiled artifact and fail
  closed on unknown evaluators or tampered hashes; reports stamp the
  ruleset hash.
- Expanded test suites: concurrency races, error-hygiene canaries, deep
  conservation fuzz, near route flow, ruleset and compiler tests.
- OpenAPI 3.1 spec (docs/openapi.json) with conformance tests; security
  review (docs/security-review.md); claims audit (docs/claims-audit.md);
  integration research record (docs/integrations/near.md); ADRs
  0004-0005; PROPOSALS.md; .env.example; near preflight and demo scripts.

### Fixed
- Unknown envelope field names are no longer echoed into 400 response
  bodies (found by the error-hygiene canary suite); scanner errors are
  bounded before they reach any response.

### Changed (2026-10-21: truth and privacy hygiene)
- Privacy: the leakage scanner, its tests, the scanner privacy test, and
  SCANNER_SPEC.md no longer contain private terms. Real terms are
  checked off-repo only, from the digest file named by
  APS_PRIVATE_DENYLIST; the published denylist holds a synthetic
  stand-in only, because a published digest of a guessable word is
  reversible. New `digest` and `scan-tree` compiler commands.
- Docs: the function-call-key hardening claim is withdrawn as
  infeasible (function-call keys cannot attach the 1 yoctoNEAR that
  NEP-141 ft_transfer requires); the planned replacement is a
  spend-policy wallet contract. The near preflight script now says so
  when it finds a function-call key.
- Docs: stale "mainnet E2E pending" statements and the SECURITY.md "no
  real funds" sentence are corrected; SECURITY.md lists "Non-goals and
  guarantees" in neutral voice; the claims audit is refreshed with
  regenerated test counts.
- Docs: ADR-0006 (prior work, commit history, and the hackathon window)
  and ADR-0007 (product direction).

### Added (2026-10-21)
- CI voice gate: scripts/voice-check.py fails on first-person plural in
  tracked Markdown (`<!-- voice-ok -->` exempts a verbatim third-party
  quote).

### Fixed (2026-10-21)
- `compile --check` is byte-stable on Windows: write_artifact pins LF
  line endings.

