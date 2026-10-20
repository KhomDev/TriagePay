# ADR 0002: Reserve-settle-refund charge timing

Status: accepted

## Context

Charging before work risks paying for a failed scan; charging after work
risks uncompensated work. Machine callers retry aggressively, so the
boundary must be idempotent.

## Decision

Work is paid by a three-phase flow: reserve moves credit from available to
reserved and creates the job; settle consumes reserved credit only from
the Running state and only against a non-zero 32-byte report hash; refund
returns reserved credit to available exactly once. Both terminal states
are final; replays return stored receipts and never move funds.

## Consequences

- A failed or timed-out scan self-heals into a refund.
- Payment success never implies analytical acceptance: the report decision
  (supported, needs_evidence, held, rejected) is orthogonal to the charge.
- Receipts are replayable audit artifacts.
