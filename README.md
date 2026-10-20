# AgentPay Sentinel

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

**Status: working demonstrator.** Mock settlement, in-memory ledger,
single dev caller. No production deployment, no mainnet keys, no live
payment processing.

## Quickstart

```sh
# Rust core, payment, and API (50 tests)
cargo test --workspace

# Python scanner (61 tests)
cd scanner && python -m pytest -q

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
  job lifecycle, scan execution, payment intents.
- `scanner/` — hostile-input normalization (embedded instructions are
  flagged, never interpreted), nine deterministic evidence gates,
  decision matrix (supported / needs_evidence / held / rejected),
  content-hashed reports.

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
crates/core      Rust prepaid ledger (dependency-free)
crates/payment   provider trait + reconciliation + mock provider
crates/api       axum HTTP API + scanner subprocess bridge
scanner/         Python scan engine (stdlib-only runtime)
demo/            demo finding envelope
scripts/         demo.sh / demo.ps1
docs/            architecture, threat model, ADRs
```

## NEAR HACKATHON track fit

`agents` (agent commerce: a metered tool an agent can buy), `security`
(spend policy, deterministic gates, auditable receipts), `business`
(prepaid metered tool spending). The payment adapter targets NEAR
Intents via the 1Click Swap API behind the `PaymentProvider` trait; the
mock provider is the default and the integration milestone is documented
in `docs/architecture.md`.

## License

MIT — see [LICENSE](LICENSE).

