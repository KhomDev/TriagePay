# ADR 0009: x402 profile on NEAR and receipt-level verification

Status: accepted (2026-11-06)

## Context

Agents expect HTTP 402 semantics (x402) rather than a bespoke
"create intent, pay, POST the hash" flow. Two facts shaped the design:

- x402 v2 already has a NEAR binding upstream (`scheme_exact_near.md`,
  commit `f5dd40efb4`): the payer signs a NEP-366 delegate action for one
  `ft_transfer`, a facilitator relayer submits it, and function-call keys
  MUST be rejected. A policy-wallet agent (prompt 02) holds only a
  function-call key, so it cannot use that method.
- The verifier of ADR-0005 inspected the transaction's top-level actions.
  A policy-wallet payment arrives as a cross-contract receipt, so that
  verifier rejected it; and a naive "find a matching event in the logs"
  check would accept events forged by any contract, or an
  `ft_transfer_call` that the token later refunds.

## Decision

1. **Receipt-level verifier** (`crates/near-adapter/src/verify.rs`). Fetch
   with `tx_status` and fall back to `EXPERIMENTAL_tx_status` on
   METHOD_NOT_FOUND (both return receipt bodies; plain `tx` does not), at
   `wait_until: FINAL`. Confirm only when the top-level status is
   `SuccessValue`, a reachable receipt executed by the asset contract
   succeeded, its body is an `ft_transfer` with 1 yoctoNEAR (never
   `ft_transfer_call`), and its logs hold exactly one `nep141`
   `ft_transfer` event paying the gateway with the intent memo and a u128
   amount. Logs from other executors are ignored. A second matching
   transfer is an orphan for operator refund. The event's `old_owner_id`
   is recorded as the payer. The `Verification` enum is unchanged.
   `parse_status` accepts only `SuccessValue` and `Failure` as final
   statuses; `SuccessReceiptId` at the top level is malformed. An RPC
   timeout is "not final yet".
2. **x402 transport** (`crates/api/src/x402.rs`): scheme `exact`, a new
   client-submitted method `extra.assetTransferMethod = "near-tx-proof"`
   with `extra.paymentFlow = "upfront"`, using the v2 headers
   `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`, `PAYMENT-RESPONSE`. The
   requirement is a fresh deposit intent whose id is the memo (request
   binding by a server-issued nonce). "Not final yet" is `402` with
   `errorReason: settlement_pending` and the transaction hash, per the
   core spec, not `202`.
3. **One funding path.** The x402 route funds through the same
   `observe_and_fund` function as `POST /payment/intents/{id}/observations`:
   pure `reconcile`, ledger deposit idempotent on `intent:<id>`. The
   x402 layer only adds the proof claim (`<network>:<tx_hash>`), the
   reservation (`x402:<intent>:<attempt>`), the scan and the settlement.
4. **Facilitator endpoints** `/x402/supported`, `/x402/verify`,
   `/x402/settle` expose the same verifier for third-party servers.
   They never sign or submit (`signers` is empty).
5. **Upstream delegate-action method: deferred.** Verifying a
   `SignedDelegateAction` is feasible offline, but settling it needs a
   relayer account and sponsorship policy. It is recorded as future work
   in `docs/protocols/x402-near.md`.

## Consequences

- Policy-wallet payments and direct transfers verify identically; the
  novelty claim is the policy-wallet client-submitted method plus receipt
  verification, never "first x402 on NEAR".
- Clients need the near-tx-proof flow (`clients/python`,
  `clients/typescript`); stock `@x402/near` clients cannot pay this
  method, and this server does not yet accept theirs.
- Proof consumption and settled responses are process-local, like the
  ledger. A restart forgets consumed proofs; durable storage is a later
  milestone and must persist both together.
- A non-archival RPC node garbage-collects old transactions within days
  (observed on mainnet, 2026-11-04). Requirements expire after
  `maxTimeoutSeconds`, which bounds how old a proof can be; evidence
  re-verification of old transactions needs the archival endpoint.
- Verification trusts the asset contract's code (upgradeable by its key
  holder), which accepting that asset already implies.

## Alternatives considered

- **Net gateway delta over all asset events** instead of binding the
  method: also defeats the refund case, but needs every refund path of
  every token right. Binding `ft_transfer` is simpler and stricter.
- **202 for "not final yet"**: rejected; the core spec defines the
  non-terminal `settlement_pending` reason.
- **A new scheme name or new headers**: rejected; the spec's own
  extension points (`assetTransferMethod`, `paymentFlow`, `extra`) are
  enough and keep standard clients' parsing intact.
