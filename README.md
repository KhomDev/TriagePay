# TriagePay

Prepaid, policy-guarded payment rails for AI agent tools — with a
privacy-preserving security-finding scanner as the first product built
on them.

An agent (or its operator) funds a prepaid balance through a payment
provider, reserves credit for a scan, and receives an idempotent report.
Every unit of credit is accounted for by an integer-only ledger: funds
are reserved before work starts, settled only against a non-zero report
content hash, and refunded on failure. A finding that fails validation
never costs anything; a completed report settles exactly its price — the
verdict never affects the charge.

**Status: working demonstrator with a real NEAR verification path.** The
ledger is in-memory, single dev caller, mock provider by default. The
NEAR adapter (APS_PROVIDER=near) funds credit only from on-chain-verified
NEP-141 deposits - read-only mainnet verification is live (status,
accounts, token metadata), and one end-to-end mainnet deposit with real
native USDC (the settlement asset) was executed and adapter-verified on 2026-10-21 (evidence in
docs/integrations/near.md). Every further on-chain transfer stays
operator-gated: scripts/near-demo.ps1 / .sh.

## Quickstart

```sh
# Rust core, payment, API, and near adapter (92 tests)
cargo test --workspace

# Python scanner (69 tests)
cd scanner && python -m pytest -q

# Knowledge compiler (37 tests, plus 1 local-only private-denylist check)
cd knowledge-compiler && python -m pytest -q

# OpenAPI conformance (4 tests, run from the repo root)
python -m pytest tests -q

# Lint the Python packages
python -m ruff check scanner knowledge-compiler

# Full product demo: funding, scan, settlement, rejection, refund
./scripts/demo.sh        # Linux/macOS
./scripts/demo.ps1       # Windows
```

No keys or external services are needed. The scanner is stdlib-only
Python; the Rust core and payment crates have zero external dependencies.

## The demo (five minutes, one command)

`scripts/demo.sh` (or `demo.ps1`) starts the API and walks the whole
product:

1. **Fund** — create a 5 USDC payment intent; the provider observes a
   matching confirmation and the account is credited exactly once.
   Create the same intent with a wrong amount and watch it never fund.
2. **Reserve** — lock 1 USDC for one `scan.v1` job.
3. **Scan** — POST the finding envelope; the Python engine runs as a
   subprocess, nine deterministic gates produce the report, and the job
   settles against the report content hash.
4. **Reject** — an invalid envelope is a 400; the job stays uncharged.
5. **Refund** — an unused reservation returns its credit.

Final balances always satisfy `deposited == available + reserved +
settled` — the invariant the randomized ledger fuzz checks after every
operation.

## Architecture

```
agent --HTTPS--> aps-api (Rust, axum) --> aps-core (Rust ledger)
                        |
                        +-- subprocess --> aps-scanner (Python, stdlib)
                        |
                        +-- PaymentProvider trait --> mock provider
```

- `crates/core` — integer money (u128 atomic units, 6 decimals),
  accounts, idempotent deposits/reservations/settlements/refunds, SKU
  price book (`scan.v1` = 1 USDC), conservation invariants. Zero
  dependencies.
- `crates/payment` — provider trait, pure reconciliation (credit funds
  only from provider-observed confirmations matching amount and asset),
  and a deterministic mock provider with finality delay, mismatches,
  expiry, and one-shot refunds. Zero dependencies.
- `crates/api` — axum HTTP surface: accounts, deposits, reservations,
  job lifecycle, scan execution, payment intents. Selects the payment
  provider (mock default; NEAR adapter via APS_PROVIDER=near).
- `crates/near-adapter` — verification-only NEAR payment provider: memo-bound
  NEP-141 deposit intents, on-chain transaction verification over JSON-RPC
  (wait_until FINAL), no signing, refunds as operator-approved proposals.
  See docs/integrations/near.md.
- `knowledge-compiler/` — compiles private knowledge sources into the
  versioned, hash-addressed, leakage-scanned ruleset the scanner executes.
- `scanner/` — hostile-input normalization (embedded instructions are
  flagged, never interpreted), nine deterministic evidence gates,
  decision matrix (supported / needs_evidence / held / rejected),
  ruleset-driven gates (compiled artifact, fail closed), content-hashed
  reports stamped with the ruleset hash.

Full details: [docs/architecture.md](docs/architecture.md),
[docs/threat-model.md](docs/threat-model.md), and the ADRs under
[docs/decisions/](docs/decisions/).

## HTTP surface

| Route | Effect |
|---|---|
| `GET /health` | liveness |
| `GET /accounts/{id}` | balances |
| `POST /accounts/{id}/deposits` | direct idempotent credit |
| `POST /payment/intents` | create a deposit intent |
| `GET /payment/intents/{id}` | intent status |
| `POST /payment/intents/{id}/observations` | poll the provider; funds only on an exactly matching confirmation |
| `POST /payment/intents/{id}/refunds` | provider refund of a confirmed intent |
| `POST /reservations` | lock credit for a job |
| `GET /jobs/{id}` | job snapshot |
| `POST /jobs/{id}/start` | begin work |
| `POST /jobs/{id}/scan` | run the scanner and settle against the report hash |
| `POST /jobs/{id}/settlements` | settle against an externally supplied hash |
| `POST /jobs/{id}/refunds` | return reserved credit |

All money crosses the wire as u128 atomic units encoded as JSON strings
plus a display decimal. Every money-moving request carries a
caller-scoped idempotency key: the same key with the same payload replays
the recorded receipt; a changed payload is a 409. One error shape
everywhere: `{"error":{"code","message"}}`.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `APS_PORT` | 8080 | API listen port (loopback only) |
| `APS_SCANNER_PYTHON` | discovered | scanner interpreter |
| `APS_SCANNER_DIR` | discovered | directory containing `aps_scanner` |
| `APS_SCANNER_TIMEOUT_SECS` | 30 | hard per-scan timeout |
| `APS_PROVIDER` | mock | `mock` or `near` (on-chain deposit verification) |
| `NEAR_NETWORK` | mainnet | network for the near provider |
| `NEAR_GATEWAY_ACCOUNT` | — | gateway account receiving deposits (near only) |
| `NEAR_ASSET_CONTRACT` | — | NEP-141 settlement asset contract (near only) |
| `NEAR_RPC_URL` | official endpoint | JSON-RPC endpoint (near only) |

## Design rules

1. Integer money only. No floats anywhere near a balance.
2. Idempotency is structural, not advisory.
3. Reserve before work; settle only against a non-zero report hash;
   refunds return credit exactly once.
4. Payment covers the scan, not the verdict.
5. Scanner input is hostile data; embedded instructions are flagged,
   never executed, and never change an outcome (enforced by an
   injection corpus test).
6. Credit funds only from provider-observed confirmations; a client
   assertion of payment has no route into the ledger.
7. Nothing private leaks: no protocol names, private paths, or corpus
   content in code, fixtures, logs, or reports (enforced by privacy
   tests on serialized output).

## Repository layout

```
crates/core          Rust prepaid ledger (dependency-free)
crates/payment       provider trait + reconciliation + mock provider
crates/api           axum HTTP API + scanner subprocess bridge
crates/near-adapter  verification-only NEAR payment adapter
knowledge-compiler/  private knowledge to opaque rulesets (stdlib-only)
scanner/             Python scan engine (stdlib-only runtime)
tests/               root-level OpenAPI conformance tests
demo/                demo finding envelope
scripts/             demo.sh / demo.ps1 / near-demo / near-preflight
docs/                architecture, threat model, ADRs, integrations, audits
```

## NEAR HACKATHON track fit

`agents` (agent commerce: a metered tool an agent can buy), `security`
(spend policy, deterministic gates, auditable receipts), `business`
(prepaid metered tool spending). The implemented v1 adapter is direct and verification-only: memo-bound
NEP-141 deposits verified on chain over JSON-RPC (no custodian, no
signing - see docs/integrations/near.md). NEAR Intents via the 1Click API
is the documented v2 for cross-chain deposits, behind the same
`PaymentProvider` trait; the mock provider remains the default.

## License

MIT — see [LICENSE](LICENSE).

