# ADR 0005: NEAR adapter shape - direct on-chain verification

Status: accepted

## Context

The gateway must fund credit only from provider-observed confirmations
(ADR-0003). Two candidate NEAR integrations exist: the NEAR Intents 1Click
API (quote → deposit address → signed intent → status polling; relayer
infrastructure, JWT auth) or direct NEP-141 transfers on mainnet with
verification over the public JSON-RPC. The operator's wallet already holds
the settlement asset on mainnet, and no relayer account/JWT exists yet.

## Decision

v1 implements **direct NEP-141 verification**:

- A deposit intent is bound to a unique memo (the intent id). The payer
  executes `ft_transfer` to the gateway account with that memo, attaching
  exactly 1 yoctoNEAR (NEP-141).
- The caller submits only a **transaction hint** (hash + signer id). The
  adapter fetches the transaction with `wait_until: "FINAL"` and
  re-verifies every fact on chain: success status, target contract,
  method, decoded arguments (recipient, memo, amount), and the attached
  deposit. The hint locates; the chain proves.
- Verification outcomes map to the existing provider vocabulary: a
  matching transfer becomes `Observation::Confirmed` with the observed
  amount (reconcile then decides Fund vs Mismatch); structural mismatches
  are client errors; a failed transaction rejects the intent; an unseen
  transaction stays pending.
- Refunds are **proposals only**: the provider returns an exact on-chain
  transfer plan marked PROPOSAL ONLY, one per intent, and never executes
  it. On-chain execution requires operator approval (the hackathon
  contract: the agent never signs).
- Intent ids embed a process tag and the synced clock, so a memo can
  never collide across restarts: one transaction can fund at most one
  intent, ever.

## Consequences

- No third-party dependency for the deposit path; works with the
  operator's wallet and the public RPC today.
- The funding invariant is preserved end to end: a client assertion has
  no path into the ledger, and the ledger's intent-keyed idempotency
  makes double funding impossible even under concurrent polls.
- Trade-off accepted: the caller supplies the tx hash (no listener). An
  indexer-based listener that removes the hint step is the post-hackathon
  upgrade; NEAR Intents 1Click remains the documented v2 for cross-chain
  deposits (docs/integrations/near.md).
