# Architecture

AgentPay Sentinel is a prepaid payment gateway for AI agent tools. The first
metered tool is a security-finding scanner: a caller deposits stablecoin
credit through a payment adapter, requests a scan, and receives an
idempotent report; every unit of credit is accounted for by an
integer-only ledger.

## Components

```
            +---------------------+
            |  agent / operator   |
            +---------+-----------+
                      | HTTPS (JSON)
            +---------v-----------+     +----------------------+
            |  aps-api (Rust)     |<--->| aps-scanner (Python) |
            |  - routes           |     | - schema             |
            |  - auth boundary    |     | - normalization      |
            |  - idempotency      |     | - gates              |
            |  - error mapping    |     | - decision matrix    |
            +---------+-----------+     +----------------------+
                      | owns                (invoked per job,
            +---------v-----------+        report hash out)
            |  aps-core (Rust)    |
            |  - Amount (u128)    |
            |  - Ledger           |
            |  - Job state machine|
            |  - conservation     |
            +---------+-----------+
                      | payment provider trait
            +---------v-----------+
            |  provider adapters  |
            |  - mock (default)   |
            |  - NEAR (planned)   |
            +---------------------+
```

## Language split

- **Rust (aps-core, aps-api)** owns everything money-critical: balances,
  reservations, settlement, refunds, idempotency, and the HTTP surface.
  The core crate is dependency-free by policy; adding a dependency
  requires an ADR.
- **Python (aps-scanner)** owns finding analysis: schema validation,
  hostile-input normalization, deterministic gates, and the report.
  It is stdlib-only at runtime. It never touches money and never
  sees an account identity.

## Money flow (one scan)

1. `deposit`: caller-scoped idempotency key; credit enters `available`.
2. `reserve`: credit moves `available -> reserved`; a job is created in
   `Reserved` state. The price of the scan SKU is fixed in the price
   book (1 USDC, 6 decimals, integer atomic units).
3. `start`: job moves to `Running`.
4. scan: the scanner consumes the normalized finding and produces a
   report plus a sha-256 content hash.
5. `settle(job, report_hash)`: settlement requires `Running` state and
   a non-zero hash; credit moves `reserved -> settled_total`.
6. `refund(job, reason)`: on failure or timeout, credit moves
   `reserved -> available` exactly once.

Retries at any step replay the recorded result; the same idempotency key
with a changed payload is a conflict, not a new operation.

## Job state machine

```
Reserved --> Running --> Settled   (terminal)
   |            |
   +--> Refunded <--+             (terminal)
```

Illegal edges are rejected with typed errors. Terminal states accept no
further transitions. Settling twice with a different report hash is a
hard error; with the same hash it is a replay.

## Trust boundaries

- **Caller -> API**: all input is untrusted. Amounts are u128 atomic
  units as JSON strings; bodies are strict-schema; malformed anything is
  400. A single dev caller is the current documented limitation; real
  authentication is a milestone.
- **API -> scanner**: finding text is hostile data. Instructions
  embedded in a finding are flagged, never interpreted, and never
  influence a gate outcome (enforced by the injection corpus tests).
- **API -> payment provider**: a client assertion never marks credit as
  funded. The provider trait observes confirmations and reconciles; the
  mock provider models finality, retries, and refunds deterministically.
- **Public repo -> private knowledge**: the scanner consumes compiled,
  versioned, opaque rulesets. No protocol names, private paths, or
  corpus content can appear in code, fixtures, logs, or reports
  (enforced by privacy tests on serialized output).

## Invariants

1. Conservation: for every account at all times, `deposited_total ==
   available + reserved + settled_total` (property-checked by a
   randomized operation fuzz).
2. Integer money only; no floats near a balance, ever.
3. Reservation before work; settlement only against a non-zero report
   hash; refunds return credit exactly once.
4. Idempotency is structural: replay cannot move funds twice.

## Deployment shape (target)

Single service: aps-api hosts the routes, holds the in-memory ledger
(a durable store is a later milestone), and invokes the scanner in-process
or as a sidecar. The payment adapter talks to a provider (mock by
default; a NEAR Intents 1Click-based adapter behind the same trait is
the integration milestone). Nothing in the core or API imports
provider-specific code.
