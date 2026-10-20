# Contributing

## Ground rules

1. Money is integer atomic units (`u128`). No floats, ever.
2. `crates/core` and `crates/payment` stay dependency-free; adding a
   dependency requires an ADR.
3. Every money-moving operation is idempotent by key and covered by a
   replay test.
4. Scanner input is hostile data: any change to normalization or gates
   must keep the injection corpus green (injections never change
   outcomes).
5. No secrets, private paths, protocol names, or private corpus content
   anywhere in code, tests, fixtures, logs, or docs.

## Workflow

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cd scanner && python -m pytest -q
```

All four must pass before a commit lands. Small commits, present tense,
scoped to one concern.

## Adding a payment provider

Implement the `PaymentProvider` trait in `crates/payment` (or an
integration crate that depends on it). The gateway funds credit only
from provider-observed confirmations that reconcile exactly — keep it
that way. See [docs/decisions/0003-idempotency-and-provider-isolation.md](docs/decisions/0003-idempotency-and-provider-isolation.md).

## Adding a gate

Gates are deterministic and content-addressed: the same envelope must
always produce the same gate result. A new gate needs Pass/Fail/
NeedsEvidence branch tests, a golden case, and a decision-matrix update
if it can reject.

