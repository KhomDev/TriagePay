# NEAR Intents 1Click API (research, 2026-10-21)

Scope: prompt 03 research item 5 (optional pillar P8). Primary sources: the docs at
`docs.near-intents.org` (Markdown page variants) and the published OpenAPI document
`https://1click.chaindefuser.com/docs/v0/openapi.yaml` (info.version **0.1.10**). The public
`GET /v0/tokens` list was fetched without a key. No quote was requested.
`docs/integrations/near.md` already records the docs entry point and SDK names; this file adds
the field-level detail.

Status legend as in `near-contract-toolchain.md`. All access dates are 2026-10-21.

## 1. Base URL, auth, endpoints

| Claim | Source | Accessed | Status |
|---|---|---|---|
| Base URL `https://1click.chaindefuser.com` (OpenAPI `servers`). | openapi.yaml | 2026-10-21 | Verified-primary |
| Auth schemes: `X-API-Key` header ("API key for partner authentication (recommended)"), `JWT-auth` bearer ("legacy, use X-API-Key instead"), and `User-Session` bearer (from `/v0/auth/authenticate`). The partner JWT comes from the Partner Dashboard (partners.near-intents.org) and is sent as `X-API-Key`; `Authorization: Bearer` is also accepted. | openapi.yaml; https://docs.near-intents.org/integration/distribution-channels/1click-api/authentication.md | 2026-10-21 | Verified-primary |
| Unauthenticated requests pay an extra **0.25%** fee on every non-`ANY_INPUT` quote; authenticated requests use the partner schedule ("typically 20 bps, or 1 bp on stablecoin and same-asset multichain routes"). | authentication page | 2026-10-21 | Verified-primary |
| Endpoints: `GET /v0/tokens`; `POST /v0/quote` (returns a unique `depositAddress`; `dry: true` simulates without one); `GET /v0/status?depositAddress=...&depositMemo=...`; `POST /v0/deposit/submit` (optional notification of the deposit tx hash); also `/v0/any-input/withdrawals`, `/v0/generate-intent`, `/v0/submit-intent`, `/v0/orders...`, `/v0/auth/{authenticate,refresh}`, `/v0/account/{balances,history}`. Quote and status accept `X-API-Key` or `JWT-auth`. | openapi.yaml paths | 2026-10-21 | Verified-primary |
| "There is no testnet version of NEAR Intents - use small amounts for test swaps." | https://docs.near-intents.org/integration/distribution-channels/1click-api/sdk.md | 2026-10-21 | Verified-primary |
| 1Click "does not take custody" of assets; they move to a quote-specific deposit address. Use is subject to the 1Click Terms of Service. | about-1click-api page | 2026-10-21 | Verified-primary |

## 2. Quote request and response

`QuoteRequest` required fields: `dry`, `swapType`, `slippageTolerance`, `originAsset`,
`depositType`, `destinationAsset`, `amount`, `refundTo`, `refundType`, `recipient`,
`recipientType`, `deadline`. Optional fields include `depositMode` (`SIMPLE` default, or `MEMO`
for chains that need a deposit memo), `connectedWallets`, `sessionId`,
`virtualChainRecipient`, `virtualChainRefundRecipient`, `customRecipientMsg`, `confidentiality`
(`public` default, `basic`, `advanced`), `referral`, `rebates`, `quoteWaitingTimeMs`, `appFees`,
`insured`. Status: Verified-primary (openapi.yaml).

| Field | Values / meaning | Status |
|---|---|---|
| `swapType` | `EXACT_INPUT`, `EXACT_OUTPUT`, `FLEX_INPUT`, `ANY_INPUT`. For `EXACT_INPUT`, a short deposit is refunded by the deadline and an excess is swapped and the surplus refunded to `refundTo`. | Verified-primary (EXACT_INPUT text); the EXACT_OUTPUT refund rules were not read in full: Unverified |
| `amount` | integer string in base units | Verified-primary |
| `recipientType` | `DESTINATION_CHAIN`, `INTENTS`, `CONFIDENTIAL_INTENTS` | Verified-primary |
| `depositType` / `refundType` | `ORIGIN_CHAIN`, `INTENTS`, `CONFIDENTIAL_INTENTS` | Verified-primary |
| `customRecipientMsg` | "**HIGHLY EXPERIMENTAL**": a message passed to `ft_transfer_call` when withdrawing to NEAR; "Otherwise, `ft_transfer` will be used." Funds are lost if the recipient does not implement `ft_on_transfer` or lacks storage. | Verified-primary |
| Asset ids | `nep141:<contract>` (and `nep245:...`, `1cs_v1:...`). The NEAR native USDC is `nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`, 6 decimals. | Verified-primary (live `/v0/tokens`) |

`QuoteResponse`: `correlationId`, `timestamp`, `signature`, `quoteRequest`, `quote`. The
`quote` object includes `depositAddress`, `depositMemo`, `chainDepositAddresses`, `amountIn`,
`minAmountIn`, `amountOut`, `minAmountOut` (with formatted and USD variants), `deadline`,
`timeWhenInactive`, `timeEstimate`, `refundFee`, `withdrawFee`. Quote and status payloads are
signed by 1Click (a "Verify Quote Signatures" guide exists; not read: the key and algorithm are
Unverified).

## 3. Status

`GetExecutionStatusResponse`: `correlationId`, `quoteResponse`, `status`, `updatedAt`,
`swapDetails`. `status` is one of `KNOWN_DEPOSIT_TX`, `PENDING_DEPOSIT`, `INCOMPLETE_DEPOSIT`,
`PROCESSING`, `SUCCESS`, `REFUNDED`, `FAILED`. `swapDetails` includes `intentHashes`,
`nearTxHashes`, `originChainTxHashes`, `destinationChainTxHashes` (each `{hash, explorerUrl}`),
`amountIn`, `amountOut`, `depositedAmount`, `refundedAmount`, `refundReason`, `slippage`,
`withdrawFee`, `referral`. Status: Verified-primary (openapi.yaml).

## 4. Supported chains and assets (live)

`GET /v0/tokens` returned 205 tokens across 37 `blockchain` values on 2026-10-21: abs, adi,
aleo, aptos, arb, avax, base, bch, bera, bsc, btc, cardano, dash, doge, eth, fogo, gnosis, hood,
hypercore, ltc, monad, movement, near, op, plasma, pol, qtc, scroll, sol, starknet, stellar, sui,
ton, tron, xlayer, xrp, zec. USDC is listed on near, eth, arb, base, sol, gnosis, pol, bsc,
aptos, op, avax, sui, stellar, monad, xlayer and hypercore. Status: Verified-primary.

## 5. SDKs

| Claim | Source | Accessed | Status |
|---|---|---|---|
| Official SDK repositories: `defuse-protocol/one-click-sdk-typescript` (npm `@defuse-protocol/one-click-sdk-typescript`, latest **0.1.26**), `defuse-protocol/one-click-sdk-go`, `defuse-protocol/one-click-sdk-rs`. | sdk page; npm registry | 2026-10-21 | Verified-primary |
| For Rust the docs say to clone the repository and use a **path dependency** (`one-click-sdk-rs = { path = "./one-click-sdk-rs" }`). The repository's `Cargo.toml` names the package `one-click-sdk-rs`, version **0.1.17**; the last push was 2026-03-12. | sdk page; `Cargo.toml` on branch `master`; GitHub API | 2026-10-21 | Verified-primary |
| A crates.io crate `one-click-sdk-rs` 0.1.17 exists, but its sole owner is a personal account, not the `defuse-protocol` organization. Its provenance is unconfirmed. | crates.io owners API | 2026-10-21 | Verified-primary (ownership); official status Unverified |
| The Rust SDK (March 2026) may lag the current OpenAPI (0.1.10): newer request fields such as `confidentiality` or `rebates` may be missing. | inferred from dates | 2026-10-21 | Unverified; compare the generated models with openapi.yaml |

## 6. Binding a payment to a Sentinel intent

| Question | Finding | Status |
|---|---|---|
| Can the quote set the recipient to the gateway account? | Yes: `recipient: "gateway.<operator>"` with `recipientType: "DESTINATION_CHAIN"` and `destinationAsset` = native USDC delivers by `ft_transfer` on NEAR (`INTENTS` would credit an intents.near balance instead). | Verified-primary (field semantics); not executed |
| Can the payer bind a memo equal to the intent id on the NEAR-side transfer? | **No such request field exists.** `depositMemo` is an origin-chain deposit memo issued by 1Click, not a destination memo. The memo 1Click puts on its outgoing `ft_transfer` is not documented. | Verified-primary (absence in QuoteRequest); destination memo content Unverified (observe one real swap) |
| Alternative bindings | (a) **Per-quote deposit address:** each quote has a unique `depositAddress`; the gateway creates the quote itself, stores `depositAddress -> intent_id`, and polls `/v0/status`. (b) **Exact destination amount:** `swapType: "EXACT_OUTPUT"` fixes `amountOut`, which permits a unique per-intent amount suffix. (c) `customRecipientMsg` with `ft_transfer_call` is experimental and needs a receiver contract: avoid. | Derived; (b) needs EXACT_OUTPUT semantics confirmed |
| Funding authority | The gateway should credit only after verifying the NEAR-side transfer (receipt-level verifier) whose tx hash appears in `swapDetails.destinationChainTxHashes`, never on `status: SUCCESS` alone. | Recommendation (matches prompt 03) |

## Implications for the build

- **Prompt 03 (P8):** the binding-by-memo plan does not work as written. Bind by the
  gateway-created quote's unique `depositAddress` (primary), cross-checked by
  `destinationChainTxHashes` plus the receipt-level verifier, with an exact `amountOut` via
  `EXACT_OUTPUT` as a secondary check.
- **Prompt 03 (P8):** there is no testnet. Every 1Click test is a real-value mainnet operation
  and belongs to prompt 09 (M-16) under the approval protocol.
- **Prompt 03 (P8):** use an `X-API-Key` partner JWT (a human obtains it; S1 applies) to avoid the
  0.25% unauthenticated fee; store it only in the off-repo `.env`.
- **Prompt 03 (P8):** prefer a thin `urllib`/`reqwest` client against the pinned OpenAPI 0.1.10
  over the lagging Rust SDK; if the SDK is used, use the git path dependency at a pinned commit,
  not the personally owned crates.io crate.
- **Prompt 09:** M-16's "recipient = gateway with a binding memo" needs rewording to "recipient =
  gateway, binding by deposit address and destination tx hash".
