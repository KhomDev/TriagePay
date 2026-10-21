# ADR 0006: Prior work, commit history, and the hackathon window

Status: Accepted (default) — organizer answer pending

Date: 2026-10-21

## Context

NEAR HACKATHON 2026 runs its development window from 2026-11-01T00:00Z to
the deadline 2026-11-14T23:59Z (judging 2026-11-16, results 2026-11-18).
One submission is allowed per team and it cannot be edited; the jury
includes AI judges. The public demo post must state that the project was
"built at NEAR HACKATHON".

This repository already has history before the window. The reviewed
baseline is commit `0711fed` (34 commits, all dated 2026-10-20 and
2026-10-21). At that commit the repository contained:

- `crates/core` (aps-core): integer-money prepaid ledger with idempotent
  deposits, reservations, settlements, and refunds, plus conservation
  invariants and a randomized ledger fuzz test.
- `crates/payment` (aps-payment): the payment-provider trait, pure
  reconciliation, and a deterministic mock provider.
- `crates/api` (aps-api): the axum HTTP surface, scanner subprocess
  bridge, concurrency and error-hygiene suites.
- `crates/near-adapter` (aps-near): verification-only NEAR provider
  (memo-bound NEP-141 deposits verified over JSON-RPC, no signing), with a
  testnet E2E (2026-10-20) and a mainnet E2E with real USDC (2026-10-21)
  recorded in docs/integrations/near.md.
- `scanner/` (aps-scanner): hostile-input normalization and nine
  deterministic, ruleset-driven evidence gates.
- `knowledge-compiler/`: compiles sources into the versioned,
  hash-addressed, leakage-scanned ruleset; one synthetic sample ruleset.
- Docs (architecture, threat model, security review, claims audit, NEAR
  integration guide, ADRs 0001-0005), OpenAPI spec and its conformance
  tests, demo scripts, and CI.
- Test suites: 92 Rust tests (plus 3 opt-in live read-only), 68 scanner,
  23 knowledge-compiler, 4 OpenAPI conformance.

The live rules page (https://nearhackathon.com/HACKATHON.md) says nothing
about work that predates the window. Whether pre-window foundation work is
allowed, and whether only in-window work is judged, is unknown until the
organizers answer. The question is drafted for the human to post in the
NEAR Discord; it is not stored in this repository.

## Options

- **(A) Disclose in the README.** Keep the full history and add a README
  section "Prior work (before 2026-11-01)" listing exactly what existed at
  commit `0711fed` (the inventory above), plus anything committed after it
  and before the window opens.
- **(B) Tag the boundary.** Keep the history and create a local tag
  `prior-work-2026-10-31` at the last pre-window commit, so judges can run
  `git diff prior-work-2026-10-31..v0.2.0` (or `git log` over the same
  range) and see exactly what was built inside the window.
- **(C) Hold feature code.** Hold all new feature code until 2026-11-01
  and use the pre-window days only for spikes in `NEAR-HACKATON/spikes/`,
  outside the submission repository. Pre-window commits in this
  repository are then limited to truth, privacy, and documentation fixes
  to the existing foundation.

## Decision

**A+B always; C for new feature code until the organizers answer.**

- A and B are applied regardless of the answer: disclosure and a
  verifiable boundary cost nothing and remove any doubt about what was
  built in the window.
- C applies to new feature code from now until the organizers answer. If
  they confirm that disclosed pre-window foundation work is fine, C can be
  lifted for the remaining pre-window days; if they say only in-window
  work counts, C stays in force until 2026-11-01T00:00Z.
- The README section (A) and the tag (B) are created at the window
  boundary, when the last pre-window commit is known. The tag is local
  until the human publishes the repository.

## Hard rule

Commit dates are never rewritten, backdated, or forged, and history is
never squashed to hide prior work. Every commit keeps its true author and
committer date. A judge must be able to trust the history as it stands.

## Consequences

- Judges see the foundation honestly disclosed and can measure in-window
  work by a single diff.
- Pre-window time goes into spikes and truth fixes, so the window opens
  with tested designs rather than untested code.
- Prompts 02-10 follow this ADR (standing rule S8).

## Open decisions

- **(a) Operator account naming in docs.** docs/integrations/near.md names
  the mainnet operator account and the gateway subaccount. Both are public
  on chain and needed to re-verify the evidence. Options: keep the names,
  or replace them with role labels in prose and keep only transaction
  hashes. Default: keep them, because judges can verify the evidence on an
  explorer. Pending the operator's answer.
- **(b) Git-history privacy scrub.** The literal workspace markers removed
  from the tracked tree by the hash-based denylist change still exist in
  earlier commits. Whether to scrub them from history before publication
  (a history rewrite that preserves author dates) is a human decision and
  is NOT performed by this change. Any such rewrite would also have to
  honour the hard rule above: no date changes, no squashing of prior work.
- **(c) Organizer answer.** Pending; when it arrives, record it here and
  update the status line.
