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
   anywhere in code, tests, fixtures, logs, or docs - not even as
   digests. Real private terms are checked off-repo only (see
   knowledge-compiler/knowledge_compiler/leakage.py and
   APS_PRIVATE_DENYLIST); tests use synthetic stand-ins.
6. Voice: the project has one independent developer. Public text uses
   first person singular or neutral voice, never first-person plural.
   `python scripts/voice-check.py` enforces this in CI; a line carrying
   `<!-- voice-ok -->` is exempt, for verbatim third-party quotes only.

## Workflow

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
(cd scanner && python -m pytest -q)
(cd knowledge-compiler && python -m pytest -q)
python -m pytest tests -q
python -m ruff check scanner knowledge-compiler
python scripts/voice-check.py
python scripts/claims-check.py .
```

All of these must pass before a commit lands; CI runs the same set plus
cargo audit and the ruleset `--check`. Small commits, present tense,
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

