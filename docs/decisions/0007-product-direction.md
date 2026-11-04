# ADR 0007: Product direction

Status: accepted

Date: 2026-10-21

## Context

The repository at commit `0711fed` is a working, tested payment gateway
with a deterministic scanner and a verified NEAR deposit path. It is
publishable, but not yet competitive: the scanner runs a synthetic
ruleset, accuracy is unmeasured, payment hardening rested on an
infeasible key design (corrected 2026-10-21, see docs/claims-audit.md),
and there is no hosted demo. The hackathon window (ADR-0006) is two
weeks, so the build-out needs one direction and an explicit cut order.

## Decision

AgentPay Sentinel is **a confidential, pay-per-call triage service for AI
security agents on NEAR**: an agent pays per call, under an on-chain
spend policy, for a deterministic triage verdict on a security finding,
and the knowledge behind the verdict is never exposed.

Pillars, in priority order (all planned; none is implemented by this
ADR):

| Pillar | What it is | Prompt |
|---|---|---|
| P1 | Spend-policy wallet contract | 02 |
| P2 | Real triage engine | 04 |
| P3 | Measured accuracy on a public human-labelled benchmark | 05 |
| P4 | x402-style payments on NEAR | 03 |
| P5 | Autonomous hunter demo | 07 |
| P6 | Hosted demo and UI | 08 |
| P7 (stretch) | Attested verdicts | 06 |
| P8 (stretch) | Cross-chain deposits via NEAR Intents | 03, optional section |

Mainnet evidence (prompt 09) and documentation (prompt 10) run alongside
the pillars rather than as pillars of their own.

### Cut order

If the build-out falls behind, drop **P8 first, then P7**. **P1, P2, P5
and P6 are non-negotiable**: without them there is no safe payment path,
no real verdict, no agent story, and nothing a judge can try. P3 and P4
are cut only by narrowing scope (a smaller benchmark, a single payment
scheme), never by dropping them silently; any narrowing is recorded in
docs/claims-audit.md.

### Tracks

Submit to **security** and **agents**. Add **privacy** only if P7
(attested verdicts) ships, because the privacy claim must rest on a
demonstrated mechanism, not on intent.

## Consequences

- Every later prompt states which pillar it serves; work that serves none
  waits until after the submission.
- Claims about a pillar stay labelled "planned" until its evidence lands
  in docs/claims-audit.md (standing rule S6).
- The track list in PROPOSALS.md follows this ADR.
