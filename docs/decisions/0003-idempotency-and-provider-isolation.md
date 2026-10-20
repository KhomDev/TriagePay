# ADR 0003: Structural idempotency and provider isolation

Status: accepted

## Context

Agent callers retry on timeouts by design. Payment providers have
asynchronous, eventually-consistent confirmation flows (quotes, deposit
addresses, status polling, refunds). Trusting any client assertion of
payment is unacceptable.

## Decision

Idempotency keys are scoped to the caller and the operation; the same key
with the same payload replays the recorded result, and the same key with a
changed payload is a conflict. Payment integration hides behind a provider
trait: the core and API import no provider-specific code. Credit is funded
only from provider-observed confirmations.

## Consequences

- Retries are free of side effects by construction.
- The mock provider can model finality, duplication, and refund flows
  deterministically in CI, and a NEAR Intents 1Click-based adapter can
  slot in without touching the ledger.
