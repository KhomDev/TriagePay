# AgentPay Sentinel

Prepaid, policy-guarded payment rails for AI agent tools — with a
privacy-preserving security-finding scanner as the first product built on them.

An agent (or its operator) deposits stablecoin credit through a payment
adapter, requests a scan of a security finding, and receives an idempotent
report. Every unit of credit is accounted for by an integer-only prepaid
ledger: funds are reserved before work starts, settled only against a
non-zero report hash, and refunded on failure or timeout.

**Status: work in progress.** Settlement is mocked; no production deployment,
no mainnet keys, no live payment processing.

## Repository layout

- `crates/core` — Rust prepaid ledger: integer money, accounts, idempotent
  deposits, reservations, settlement, refunds, conservation invariants.
- `crates/api` — HTTP API over the ledger (Rust, axum).
- `scanner` — Python finding-scan engine: hostile-input normalization,
  deterministic evidence gates, decision pipeline, synthetic test corpus.
- `docs/` — architecture, threat model, integration notes.

## Quickstart

```sh
# Rust core + API
cargo test --workspace

# Python scanner
cd scanner
python -m pytest
```

No external services or keys are required for tests; everything runs on
synthetic fixtures and an in-memory ledger.

## Design rules

1. Integer money only. No floats anywhere near a balance.
2. Idempotency is structural, not advisory: same key + same payload replays
   the recorded result; same key + different payload is an error.
3. Reservation before work; settlement only against a non-zero report hash;
   refunds return reserved credit to the payer.
4. Scanner input is hostile data. Embedded instructions are flagged, never
  executed, and never change an outcome.
5. Nothing private leaks: no protocol names, no private corpus content, no
   host paths, no credentials in code, fixtures, logs, or docs.

## NEAR HACKATHON track fit

`agents` (agent commerce), `security` (spend policy and receipts),
`business` (metered autonomous tool spending). The payment adapter targets
NEAR Intents via the 1Click Swap API behind a provider trait; a mock provider
is the default.

## License

MIT — see [LICENSE](LICENSE).
