# Proposals: AgentPay Sentinel After the Hackathon

Recommendations for turning the current milestone into a world-class
project. Grounded in what exists today (see docs/claims-audit.md for the
verified state); every roadmap item names its first PR-sized step.

## 1. Honest current state (one paragraph)

A working, tested payment gateway for metered agent tools: an
integer-only prepaid ledger with structural idempotency (92 Rust tests
across four crates), a deterministic hostile-input scanner whose nine
gates now execute from a compiled, hash-addressed, leakage-scanned
ruleset (91 Python tests), a NEAR adapter that funds credit only from
on-chain-verified NEP-141 deposits (verified against mainnet read-only;
the end-to-end deposit run is operator-gated and pending), and docs whose
claims are audited line by line. What it is not: durable, authenticated,
rate-limited, or signed - all documented as limitations, none hidden.

## 2. Hackathon positioning

Primary tracks, in priority order:

1. **agents** (Agent Economy) - the core story: an agent prepays credit
   and buys a bounded tool call. The demo IS the track: fund -> reserve ->
   scan -> settle -> receipt, with retry-idempotency proving it is
   machine-callable.
2. **security** (AI Security & Defense) - spend policy and payment
   integrity for agents: deterministic gates, one-charge idempotency,
   on-chain deposit verification (client assertions fund nothing), and
   the NEAR-native hardening angle below.
3. **business** (Autonomous Business) - metered tool spending with
   auditable receipts: a business could sell API-shaped services to
   agents on exactly these rails.
4. **privacy** (recommended addition) - the differentiator: the
   private-knowledge compiler sells intelligence without exposure -
   opaque, hash-addressed rulesets compiled under a leakage scanner. No
   other submission in this space has a privacy boundary enforced by a
   compiler.

For AI judges (the jury includes them): keep README sections structured
with exact commands, keep docs/claims-audit.md as the claim-evidence
index, and state limitations in the same breath as capabilities.

## 3. NEAR-native hardening (the security track's ace)

Correction (2026-10-21): an earlier version of this section proposed a
gateway hot account whose function-call access key is restricted to
ft_transfer on the settlement asset. That is infeasible: function-call
keys cannot attach a deposit (docs.near.org access keys, accessed
2026-10-21: function-call keys "do not allow to attach NEAR tokens to the
call", https://docs.near.org/protocol/accounts-contracts/access-keys), and
NEP-141 ft_transfer requires exactly 1 yoctoNEAR attached, precisely so
that function-call keys cannot call it.

The replacement (planned, prompt 02) is a **spend-policy wallet
contract**. The agent holds a function-call key for the wallet's `pay`
method, which takes zero deposit and is therefore callable by such a key;
the contract checks the on-chain spend policy (allowed receivers, per-call
and per-period caps) and itself performs the ft_transfer with the required
1 yoctoNEAR. That turns "the agent process can move money" into "the agent
process can move one asset, to allowed receivers, within caps" - the
NEAR-native expression of least privilege, and a judge-visible property.
Details: docs/integrations/near.md, "Security-track angle". The operator
creates any key; the repository never holds one.

## 4. Engineering roadmap (prioritized)

| # | Item | Why | First PR |
|---|---|---|---|
| 1 | Durable ledger (SQLite/Sled behind a trait in aps-api) | restarts currently lose state; the #1 credibility gap | a durable-ledger ADR + event-sourced append log + replay test |
| 2 | Per-caller authentication (bearer keys or ed25519-signed requests) | the single dev caller is the biggest real security limitation | caller registry + per-caller idempotency scope (already structural) |
| 3 | Listener-based deposits (replace tx-hash hints) | removes the client-supplied hint step; the gateway watches for memo-matching transfers | block-scanning worker behind the provider trait |
| 4 | NEAR Intents 1Click adapter v2 | cross-chain deposits (USDC from any chain) without custodians; official Rust SDK exists | quote->depositAddress->status flow behind PaymentProvider |
| 5 | HTTP rate limits + per-account quotas | queue-exhaustion row becomes fully closed | tower middleware + quota tests |
| 6 | Signed, versioned releases + cargo-audit/deny in CI | supply-chain credibility | CI job (the audit step is already specced) |
| 7 | cargo-fuzz + Kani on the ledger | the conservation invariant deserves machine-checked status | one fuzz target on deposit/reserve/settle/refund |
| 8 | Multi-asset price book + fee accounting | today exactly one SKU at one price | pricebook ADR + multi-asset tests |
| 9 | Real knowledge compilation run (private side) | the synthetic ruleset proves the machinery; the real one proves the product | operator-side compile with the leakage scanner gating every commit |
| 10 | OpenAPI-generated client SDKs | agents integrate faster | generate TS/Rust clients from docs/openapi.json |

## 5. Trust and credibility plan

- Keep docs/claims-audit.md green in CI: a doc claim without evidence is
  a build failure.
- External review before any real funds: a paid audit of aps-core (it is
  small, dependency-free, and the money-critical core - reviewable in
  hours, not weeks).
- Bug bounty even at small size: the ledger + verification path is the
  attack surface; a public program signals seriousness.
- Reproducible builds and signed artifacts for the compiled rulesets
  (the compiler is deterministic; add a signing key ceremony).

## 6. Risks and mitigations

| Risk | Honest assessment | Mitigation |
|---|---|---|
| NEAR integration depth vs judges' expectations | read-only mainnet verification is live; the deposit E2E is not yet executed | run scripts/near-demo.ps1 with the operator's approval before the deadline; record the tx hash in the demo evidence |
| Private knowledge cannot be demonstrated publicly | true by design | demo the synthetic pipeline + leakage-scanner tests; the guarantee is the product, not the corpus |
| Single-operator custody | the gateway account is one wallet | spend-policy wallet contract (section 3, planned); later, a multisig (Trezu/Sputnik) as the deposit account |
| In-memory state | restarts lose credit | roadmap item 1; document loudly until then |
| One-shot submission errors | project name/email must be unique across the hackathon | draft the payload early, human sends, requestId reused on retry |
| Pre-window commits | rules are silent on pre-window work | docs/decisions/0006-prior-work-and-window.md: disclose prior work in the README, tag the window boundary, hold new feature code until the organizers answer; commit dates are never rewritten |

## 7. Submission checklist (from HACKATHON.md)

- Repository public before 2026-11-14T23:59Z (human action; the repo has
  no remote yet).
- X demo post with video/screenshots, stating the project was built at
  NEAR HACKATHON (human posts; the near-demo transcript is the script).
- Submission payload drafted for human confirmation: projectName (2-80,
  unique), leadName, email (unique), size, tracks (agents, security,
  business, privacy), nearAddress (mainnet payout account), projectUrl,
  demoUrl, idea (20-2000 chars), consent read verbatim.
- requestId generated once and reused on any retry; payload never
  committed (it contains personal data).

## 8. The one-sentence pitch

AgentPay Sentinel lets an AI agent prepay for a bounded, deterministic
security scan with on-chain-verified USDC and walk away with a
hash-addressed receipt - and the knowledge behind the scan is sold
without ever being exposed.
