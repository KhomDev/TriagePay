# Threat Model

Asset: prepaid credit, metered-tool receipts, and the privacy of the
compiled knowledge behind the scanner.

## Abuse cases and controls

| # | Threat | Vector | Control | Verified by |
|---|--------|--------|---------|-------------|
| 1 | Double spend | replay the same deposit request | caller+key idempotency; replay returns the stored receipt | ledger replay tests |
| 2 | Key collision attack | reuse a key with a changed payload to alter a recorded result | payload mismatch is a hard conflict error | conflict tests |
| 3 | Reservation race | concurrent reserves spend the same credit | single-writer ledger behind RwLock; sufficiency checked atomically with the debit | API concurrency tests (crates/api/tests/concurrency.rs: reservation race, double-poll funding, concurrent scans) |
| 4 | Free work | settle without a report | settlement requires Running state and a non-zero 32-byte hash | settle validation tests |
| 5 | Refund theft | refund a settled job, or refund twice | Settled is terminal; Refunded is terminal; replay returns the stored receipt | refund transition tests |
| 6 | Prompt injection through a finding | instructions embedded in title, markdown, code comments, JSON | flag-only detection; gates are deterministic and content-addressed; corpus test asserts identical outcomes with injections stripped | injection corpus tests |
| 7 | Taxonomy extraction | probe the scanner to reconstruct private rule structure | gate outcomes are public, coarse, and content-hashed; no private labels, paths, or identifiers in serialized output | privacy scan tests |
| 8 | Payment spoofing | client asserts "paid" | credit is funded only from provider-observed confirmations; client assertions are never sufficient | payment_flow, near-adapter provider_rpc (memo/recipient/contract/deposit/amount mismatch matrix), and near_flow route tests |
| 9 | Quote manipulation | stale or altered quote honored at settlement | quote signature verification and expiry before execution; persisted quote state | adapter tests (milestone) |
| 10 | Account enumeration | harvest account ids via error differences | uniform 404 semantics for unknown accounts and jobs | API tests |
| 11 | Queue exhaustion | flood reservations to lock credit | bounded reservation quotas per account (milestone); refunds on timeout | quota tests (milestone) |
| 12 | Log leakage | finding text or secrets in logs | no request-body logging; redacted structured events | error-hygiene canary suite (crates/api/tests/error_hygiene.rs - found and fixed attacker field-name reflection into 400 bodies) |
| 13 | Oversized payloads | memory pressure via huge fields | strict per-field length limits, evidence count caps | schema tests |
| 14 | SSRF via evidence URLs | scanner fetches attacker URLs | the engine never fetches URLs; evidence is opaque data | design; no fetch code exists |

## Residual risks (accepted for the hackathon build)

- In-memory ledger: process restart loses state. Acceptable for the
  demonstrator; a durable store is the first post-hackathon milestone.
- Single dev caller: no per-caller authentication yet. Documented
  limitation of the milestone, tracked for the auth milestone.
- No rate limiting at the HTTP layer yet (bounded by test usage).
- The NEAR adapter verifies deposits on demand (caller-supplied transaction
  hash hints) rather than via a listener, and trusts its configured RPC
  endpoint; both are documented in docs/integrations/near.md with the
  listener as the post-hackaton upgrade.

## Out of scope by design

- Mainnet key handling: the repo contains no keys and no mainnet calls.
- Arbitrary URL fetching: the scanner never dereferences evidence URLs.
- Real-world target analysis: all fixtures are synthetic.
