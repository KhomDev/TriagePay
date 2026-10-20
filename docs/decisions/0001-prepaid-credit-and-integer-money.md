# ADR 0001: Prepaid credit with integer money

Status: accepted

## Context

The gateway meters agent-tool calls that cost small, fixed amounts. A
post-paid billing model would require invoicing, trust, and dunning — none
of which a machine caller can participate in. Floating-point money is
excluded by policy.

## Decision

Credit is prepaid: callers deposit before they can reserve work. All
amounts are u128 atomic units at 6 decimals (USDC family). The core crate
has no dependencies; money operations are checked and return typed errors.

## Consequences

- No negative balances are representable; insufficiency is a typed error.
- Conservation is a structurally checkable invariant and is fuzz-tested.
- Decimal widths are explicit, so cross-asset mistakes surface at parse
  time, not at settlement time.
