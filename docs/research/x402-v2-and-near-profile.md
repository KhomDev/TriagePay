# x402 v2 and NEAR (research, 2026-10-21)

Scope: prompt 03 research items 1-2. This covers the canonical x402 v2 specification, its HTTP
transport, the `exact` scheme, network identifiers, the facilitator API, and the NEAR binding.
That binding **already exists upstream**, which changes the plan (see section 5).

Status legend as in `near-contract-toolchain.md`. All access dates are 2026-10-21.

## 1. Canonical location and version

| Claim | Source | Accessed | Status |
|---|---|---|---|
| The canonical repository is `x402-foundation/x402` (default branch `main`, last push 2026-10-15). `coinbase/x402` is still live with the same description and is the historical home. | https://github.com/x402-foundation/x402 ; https://github.com/coinbase/x402 (GitHub API) | 2026-10-21 | Verified-primary |
| The core spec is `specs/x402-specification-v2.md` ("Protocol Version: 2"). Its version history lists v2.0 dated 2025-12-9. The file's latest change was commit `e187dda1ef` (2026-08-31). | https://github.com/x402-foundation/x402/blob/main/specs/x402-specification-v2.md ; commits API for that path | 2026-10-21 | Verified-primary |
| Transports are split by version: `specs/transports-v2/{http,mcp,a2a}.md` and `specs/transports-v1/...`. Schemes live in `specs/schemes/<scheme>/scheme_<scheme>_<network>.md`. | repository tree | 2026-10-21 | Verified-primary |
| Pin for implementation: spec v2, `main` at commit `e187dda1ef` for the core document and `f5dd40efb4` for the NEAR scheme file. | derived | 2026-10-21 | Recommendation |

## 2. HTTP transport v2 (headers)

| Header | Direction | Content | Source / status |
|---|---|---|---|
| `PAYMENT-REQUIRED` | server to client, with HTTP `402` | base64-encoded JSON `PaymentRequired` | `specs/transports-v2/http.md`, Verified-primary |
| `PAYMENT-SIGNATURE` | client to server, on the retry | base64-encoded JSON `PaymentPayload` | same, Verified-primary |
| `PAYMENT-RESPONSE` | server to client, on `200` (or `402` on failure) | base64-encoded JSON `SettlementResponse` | same, Verified-primary |
| `EXTENSION-RESPONSES` | facilitator to resource server only (sidechannel) | base64 JSON keyed by extension name; "not forwarded to buyers" | core spec 7.2.1, Verified-primary |

- HTTP status mapping: `402` payment required or payment failed, `400` malformed payload or
  requirements, `500` server error, `200` success. "All x402 protocol information is
  communicated through headers"; bodies are a server concern. (transports-v2/http.md, Verified-primary)
- v1 used different names: `X-PAYMENT` and `X-PAYMENT-RESPONSE`, with requirements in the 402
  JSON body (`specs/transports-v1/http.md`, Verified-primary). The secondary-source header names
  quoted in prompt 03 are therefore correct for v2.
- Version negotiation: every object carries `x402Version` (must be `2` for v2), and the HTTP
  header names differ between v1 and v2, so a server can tell the versions apart from the
  header present. The spec defines no separate negotiation handshake. (Derived from the core
  spec and both transport files.)

## 3. Core types (spec section 5)

**`PaymentRequired`:** `x402Version` (required, 2), `error` (optional string), `resource`
(required `ResourceInfo`), `accepts` (required array of `PaymentRequirements`), `extensions`
(optional object).

**`PaymentRequirements`** (each `accepts[]` entry):

| Field | Required | Meaning |
|---|---|---|
| `scheme` | yes | scheme id, for example `exact` |
| `network` | yes | CAIP-2 identifier |
| `amount` | yes | atomic units, string |
| `asset` | yes | token contract address (or ISO 4217 code for fiat) |
| `payTo` | yes | recipient address or role constant |
| `maxTimeoutSeconds` | yes | maximum time for payment completion |
| `extra` | no | scheme-specific; **reserved protocol keys `assetTransferMethod` and `paymentFlow`** |

**`ResourceInfo`:** `url` (required), optional `description`, `mimeType`, `serviceName` (max
32 chars), `tags` (max 5), `iconUrl`.

**`PaymentPayload`:** `x402Version` (required), `resource` (optional), `accepted` (required: the
chosen `PaymentRequirements`), `payload` (required, scheme-specific object), `extensions`
(optional). Clients echo extension info and may add to it, never delete it.

**`SettlementResponse`:** `success` (required), `errorReason` (optional), `payer` (optional),
`transaction` (required; empty string if nothing was broadcast; non-empty when `errorReason`
is `settlement_pending`), `network` (required), `amount` (optional), `extensions` (optional).

**`VerifyResponse`:** `isValid` (required), `invalidReason`, `payer`, `extensions`, `extra`.

Source: core spec sections 5.1-5.4, Verified-primary.

## 4. Schemes, payment flows, facilitator API

| Claim | Source | Accessed | Status |
|---|---|---|---|
| `exact` "transfers a specific amount of funds from a client to a resource server". Default flow `authorization` (verify, resource, settle); `upfront` (settle, resource, respond) is allowed and is required when the payment is already submitted. A third flow, `escrow`, exists for other schemes. | `specs/schemes/exact/scheme_exact.md`; core spec 6.1 | 2026-10-21 | Verified-primary |
| `exact` defines two asset-transfer families: **facilitator-submitted** (client signs, facilitator submits) and **client-submitted (payment proof)** ("The client submits the payment itself and presents a proof of it"). Client-submitted methods MUST use `upfront`. | scheme_exact.md | 2026-10-21 | Verified-primary |
| Client-submitted requirements: advertise the instrument in `payTo` or `accepts[].extra` with its validity window; carry the proof in `PaymentPayload.payload`; **bind** the payment to the request (a per-request instrument, a server-issued nonce carried in the payment, a payer signature over the requirements, or a payee commitment); claim the proof atomically before the resource runs; use the consumption key "CAIP-2 network identifier joined to the network's canonical payment identifier"; declare retention, amount handling (under / over payment), finality, and failure disposition. | scheme_exact.md | 2026-10-21 | Verified-primary |
| `/settle` for client-prepaid methods "MAY bind a payment proof to the request (for example, consuming a challenge or marking a transaction as used) after read-only observation". | core spec 7.2 | 2026-10-21 | Verified-primary |
| Facilitator API: `POST /verify` (read-only; body `{x402Version, paymentPayload, paymentRequirements}`; returns `VerifyResponse`), `POST /settle` (same body; returns `SettlementResponse`), `GET /supported` (returns `{kinds: [{x402Version, scheme, network, extra?}], extensions: [], signers: {"<caip2 pattern>": [addresses]}}`). | core spec 7.1-7.3 | 2026-10-21 | Verified-primary |
| Standard error codes include `insufficient_funds`, `invalid_network`, `invalid_payload`, `invalid_payment_requirements`, `invalid_scheme`, `unsupported_scheme`, `invalid_x402_version`, `invalid_transaction_state`, `unexpected_verify_error`, `unexpected_settle_error`, and the non-terminal `settlement_pending`. | core spec 9 | 2026-10-21 | Verified-primary |
| Networks use CAIP-2 `namespace:reference` (for example `eip155:8453`); "Non-blockchain networks are encouraged to follow the CAIP-2 format". | core spec 11.1 | 2026-10-21 | Verified-primary |

## 5. NEAR upstream: the `exact` scheme on NEAR exists

| Claim | Source | Accessed | Status |
|---|---|---|---|
| `specs/schemes/exact/scheme_exact_near.md` was added by commit `f5dd40efb4` on 2026-05-16 ("Specs: add NEAR v2 exact payment scheme (#1459)"). It "supports **x402 v2 only**". | file and commits API | 2026-10-21 | Verified-primary |
| Networks: "`near:mainnet`" and "`near:testnet`" ("NEAR networks MUST use CAIP-style identifiers"). | same file | 2026-10-21 | Verified-primary |
| Mechanism: facilitator-submitted and sponsored. The client signs a NEP-366 `SignedDelegateAction` authorizing **exactly one `ft_transfer`**; the payload is `{"signedDelegateAction": "<base64 borsh>"}`; a facilitator-selected relayer submits it and pays gas and the 1 yocto deposit. No scheme-specific `extra` is required. | same file | 2026-10-21 | Verified-primary |
| Requirements example: `{"scheme":"exact","network":"near:testnet","amount":"1000000","asset":"usdc.testnet","payTo":"merchant.testnet","maxTimeoutSeconds":60}`. `asset` = NEP-141 contract account id; `payTo` = recipient account id. | same file | 2026-10-21 | Verified-primary |
| Verification MUSTs include: exact match of `asset`, `payTo`, `amount`; the delegate action's receiver = asset; one `FunctionCall` named `ft_transfer`; deposit exactly 1 yocto; timeout mapped as `timeoutBlocks = max(1, ceil(maxTimeoutSeconds / 1))` checked against `max_block_height`; nonce > the on-chain access-key nonce; signature checked for ed25519 or secp256k1; preflight `view_account`, `view_access_key`, `ft_balance_of`, `storage_balance_of(payTo)` at `final`. | same file sections 1-9 | 2026-10-21 | Verified-primary |
| The upstream spec states: "Standard NEAR `FunctionCall` access keys MUST be rejected for this `ft_transfer` flow" because they cannot attach the 1 yocto deposit. Only **FullAccess** payer keys work. | same file section 8 | 2026-10-21 | Verified-primary |
| Settlement waits for the inner `ft_transfer` receipt with `tx` or `EXPERIMENTAL_tx_status` and `wait_until: "FINAL"`, and returns `success: true` "only when the inner `ft_transfer` receipt's status is `SuccessValue`". Duplicate settlement must be prevented with an in-memory cache keyed on the payload bytes (`duplicate_settlement`). | same file | 2026-10-21 | Verified-primary |
| Reference implementation: npm package `@x402/near` **2.28.0** (published 2026-09-29; `typescript/packages/mechanisms/near`) with client, resource-server and facilitator classes (`ExactNearScheme`). It depends on `@near-js/*` ^2.5.1 and `borsh` 1.0.0. | https://registry.npmjs.org/@x402/near ; package README and package.json | 2026-10-21 | Verified-primary |
| Third-party payer skill: `NearDeFi/agent-payments-skill` ("x402 Pay") is payer-side, settles in USDC on Base, and is funded via NEAR Intents. | https://github.com/NearDeFi/agent-payments-skill README | 2026-10-21 | Verified-primary |

### CAIP-2 namespace registration

| Claim | Source | Accessed | Status |
|---|---|---|---|
| ChainAgnostic `namespaces` has **no `near` directory** yet. | https://github.com/ChainAgnostic/namespaces (contents API) | 2026-10-21 | Verified-primary |
| PR #203 "Add NEAR Protocol namespace (near) with CAIP-2 profile" (opened 2026-09-14) is **open, no reviews**. It proposes `near:mainnet` / `near:testnet`, defined as "`near:` + the nearcore `chain_id` string". Issue #106 "CAIP-2 for NEAR" (2024-03-01) is open. | https://github.com/ChainAgnostic/namespaces/pull/203 ; issue search | 2026-10-21 | Verified-primary |
| Conclusion: `near:mainnet` is the de facto identifier (x402 upstream, PR #203), but it is **not yet a registered CAIP-2 namespace**. Docs should say "CAIP-2-style, registration pending (ChainAgnostic namespaces PR #203)". | derived | 2026-10-21 | Verified-primary (inputs) |

## 6. What this means for the TriagePay "NEAR profile"

The planned flow (the client pays first with `ft_transfer` or `wallet.pay`, then retries with
`{network, intent_id, tx_hash, signer_id}`) is **not** the upstream NEAR `exact` mechanism. It is
a **client-submitted (payment proof)** method, which the `exact` scheme explicitly permits as a
family. The policy-wallet path cannot use the upstream mechanism at all: the upstream delegate
action must be a single `ft_transfer` signed by a FullAccess key, and the wallet design gives the
agent only a function-call key to `pay`.

Extension points that avoid forking core semantics:

1. **`accepts[].extra.assetTransferMethod`** (protocol-reserved key; values are
   mechanism-defined). Offer two `accepts[]` entries for one resource:
   - the upstream mechanism (default `assetTransferMethod`, `paymentFlow` `authorization`),
     for FullAccess payers, implemented with or compatible with `@x402/near`;
   - a proposed client-submitted method, for example `assetTransferMethod: "near-tx-proof"`, with
     `extra.paymentFlow: "upfront"` (MUST be present because it is not `authorization`).
2. **Request binding** through a server-issued nonce carried in the payment: the
   `ft_transfer` memo equals the server-issued intent id (`extra.memo`). This satisfies the
   "server-issued nonce carried in the payment" binding option.
3. **Proof in `PaymentPayload.payload`**, for example `{"txHash": "...", "signerId": "...",
   "intentId": "..."}`. The canonical consumption key is `near:mainnet:<tx_hash>` (CAIP-2 joined
   to the transaction hash), claimed atomically before the resource runs.
4. **Finality declaration:** the finality boundary is `wait_until: FINAL` plus a successful token
   receipt. While not final, settle returns an error naming the unmet condition (the spec's
   non-terminal `settlement_pending` requires a broadcast hash, which the client supplies).
5. **Amount handling:** state exact-match semantics. Under- or over-payment is an orphan deposit,
   refunded by an operator-approved transfer (the existing refund-proposal path).
6. **Facilitator endpoints** `/verify`, `/settle`, `/supported` with the core request/response
   bodies; `/supported` lists `{"x402Version":2,"scheme":"exact","network":"near:mainnet"}` and
   can carry the method list in `kinds[].extra`.

Mark the client-submitted method "proposed profile, not upstream". Proposing it upstream as an
additional `assetTransferMethod` in `scheme_exact_near.md` is the natural route.

## Implications for the build

- **Prompt 03:** headers confirmed: `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`,
  `PAYMENT-RESPONSE`, each base64 JSON; pin x402 v2.
- **Prompt 03:** rewrite the novelty claim. A NEAR scheme, a NEAR facilitator implementation
  (`@x402/near`) and `near:*` network ids already exist upstream. The defensible novelty is the
  client-submitted proof method with receipt-level verification, and policy-wallet payments.
- **Prompt 03:** keep `exact` as the scheme name. Distinguish the tx-proof path with
  `extra.assetTransferMethod` plus `extra.paymentFlow: "upfront"`; do not invent a new scheme
  name or new headers.
- **Prompt 03:** optionally implement the upstream delegate-action method as well, so standard
  x402 NEAR clients can pay TriagePay. The gateway would then need a relayer account and the
  section 8-10 checks; this is a scope decision for ADR-0009.
- **Prompt 03:** the 402-vs-202 question for "not final yet" is answered by the spec: under
  `upfront`, settle returns an error naming the unmet condition (HTTP `402` with
  `PAYMENT-RESPONSE`, `errorReason: "settlement_pending"`, and the transaction hash).
- **Prompt 07:** an agent holding only a policy-wallet function-call key cannot use upstream
  `@x402/near` clients; it needs the TriagePay client for the tx-proof method.
