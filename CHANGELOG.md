# Changelog

All notable changes to this project are documented here. The format
follows Keep a Changelog; the project has not shipped a tagged release.

## [Unreleased]

### Changed
- Hosted demo redesign: a midnight palette with colour only for paid,
  refused and held; Geist throughout, monospace only for chain strings;
  "Judge in 60 seconds" is a numbered sequence in the hero with the live
  controls; the attack path is one console with the contract's decision
  set large; the ticker says why each payment was refused and to whom;
  "Drained by attackers" replaces the value-protected figure; a mainnet
  USDC x402 card; owner tools grouped in an owner console; stale review
  holds are cleared automatically. `app.min.js` is built by
  `npm run build` and a test fails when it is stale.

### Security
- Remediated the red-team review of 2026-11-13 (record:
  `docs/security/redteam-2026-11-13.md`). Paid API: reservations are
  priced server-side and bound to the caller; bare transaction hints no
  longer pin state; the JSON-RPC read runs outside the provider lock;
  expiry follows the funding block's time. Engine: Stage B fits inside the
  scan deadline, the number regex is linear, compile-private requires the
  private denylist. Hosted demo: owner actions are limited to the
  allowlisted payee, policy apply needs an owner token and a confirmed
  diff, restore keeps the chain policy, signing POSTs are same-origin JSON,
  model calls and the agent allowance are budgeted. CI actions are pinned.

### Added
- Hosted judge demo (`demo/live`). A page deployable to Vercel where a
  judge runs a prompt-injected payment, an over-limit payment, and a
  flagged-payee payment on NEAR testnet and gets explorer links for the
  refusals and for an allowed payment. A pre-payment risk check classifies
  the payee (allow, warn, or block) from testnet account facts. The same
  page shows today's spend against the daily limit and a Revoke button.
  Signing keys are environment variables.

### Changed
- The product is renamed from AgentPay Sentinel to TriagePay (ADR-0013).
  Crate, package and environment-variable names keep the `aps` prefix.

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

### Added (2026-11-11)
- Agent demo (`agents/demo/`): one script that sets up throwaway testnet
  accounts, deploys the test token and the policy wallet, and runs an
  agent that pays per call through x402 with a function-call key. The
  recorded run is `docs/demo/transcript-2026-11-11.md`: four wallet-paid
  calls, one direct payment, and the wallet's refusals (payee, per-call
  limit, daily limit, owner method, revoked agent).
- `scripts/x402-demo.py`: the 402 exchange against the mock provider, with
  no keys and no chain.
- `GET /payment/intents/{id}` lists orphaned transfers.
- ADR-0011 (attested verdicts cut); ADR-0006 records the rules' answer on
  the hackathon window.

### Changed (2026-11-11)
- README rewritten around what can be run and verified; claims audit
  regenerated; architecture, threat model and security review brought up
  to the current code.
- `scripts/testnet/x402-e2e.ts` removed: it paid in `wrap.testnet`, which
  emits no NEP-297 events and therefore can never verify. The agent demo
  replaces it.

### Fixed (2026-11-11: money-path review)
- NEAR provider: a failed transaction hinted at a pending intent no
  longer closes it. Nothing binds a failed transaction to an intent, so
  any failed transaction on chain could be used to make a requirement
  unfundable while its payer's tokens had already moved.
- NEAR provider: a transfer of the wrong amount carrying the memo is
  recorded as an orphan and no longer closes the intent; the exact
  payment still funds it.
- NEAR provider: intent ids, which double as payment memos and as the
  x402 request binding, end in a 128-bit nonce from the operating
  system. They were a process id, a clock and a counter, so the next
  memo could be computed from the last.
- API: `POST /accounts/{id}/deposits` and `POST /jobs/{id}/settlements`
  answer 403 `dev_only_route` unless the mock provider is active. With
  the NEAR provider the first minted spendable credit with no payment.
- Policy wallet: the token allowlist holds at most one token. Limits are
  plain integers in the token's atomic units, so two allowlisted tokens
  with different decimals let a limit sized for one move the other up to
  the same number.

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
- Native USDC (17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1)
  is the settlement asset everywhere: the NEAR integration docs and
  .env.example, the near-preflight `--asset` fallback, the deposit
  waiter's known-contract list (native first), and the opt-in live
  read-only test (asserts symbol USDC, 6 decimals; keeps a check that
  legacy bridged USDC.e reports symbol USDC.e). The 2026-10-20 entry
  above that mentions USDC.e metadata is history.

