# Research index (2026-10-21)

Verified research for the NEAR HACKATHON build-out (prompts 02-09). Every document uses one
row per fact (claim, source URL, access date, status) and ends with "Implications for the
build". Facts already in `docs/integrations/near.md` are referenced, not repeated.

| File | Covers | Main consumer |
|---|---|---|
| [near-contract-toolchain.md](near-contract-toolchain.md) | near-sdk / cargo-near / near-workspaces versions, rustc-wasm compatibility, Windows sandbox support, access-key rules, NEP-141/145/297, callbacks, gas, live storage price | prompt 02 |
| [nep141-events-and-receipts.md](nep141-events-and-receipts.md) | `ft_transfer` event fields, `tx` / `EXPERIMENTAL_tx_status` shape at FINAL, status enums, real mainnet receipt tree, spoofing and refund pitfalls | prompt 03, 09 |
| [x402-v2-and-near-profile.md](x402-v2-and-near-profile.md) | x402 v2 types, headers, facilitator API, the upstream NEAR `exact` scheme, CAIP-2 status, extension points for a tx-proof method | prompt 03, 07 |
| [near-ai-cloud.md](near-ai-cloud.md) | NEAR AI Cloud API, live model catalogue and prices, attestation report and nonce, TDX/NRAS verification, response signatures, what is not attested | prompt 04, 06 |
| [near-intents-1click.md](near-intents-1click.md) | 1Click base URL, auth, quote/status fields, chains, SDKs, binding options | prompt 03 (P8), 09 |
| [nep413-signed-messages.md](nep413-signed-messages.md) | NEP-413 payload, tag bytes, verification steps, key checks | prompt 08 |
| [rpc-endpoints.md](rpc-endpoints.md) | public RPC status, providers, `wait_until`, method-name drift between mainnet and testnet | prompt 03, 08, 09 |

## Ten build-critical facts

1. The near-workspaces sandbox **does not run on native Windows** (near-sandbox supports only
   Linux x86_64/aarch64 and macOS arm64). Use WSL2 or Linux CI.
2. Pin `near-sdk = "=5.29.1"`. It declares protocol 84, so cargo-near 0.21.1 applies **no
   rustc ceiling** and rustc 1.98 works. 5.30.0-rc.1 targets protocol 88, but mainnet is on 86.
3. Live `storage_amount_per_byte` = 1e19 yocto, which is **1 NEAR per 100 KB**.
4. Function-call keys: exactly one `FunctionCall` action, zero deposit, receiver and method
   match. The policy-wallet design is valid; owner methods with `assert_one_yocto` are
   unreachable from agent keys.
5. Gas is prepaid at 0.001 NEAR/Tgas (10x the burn price) and refunded. Size allowances and
   liquid balances accordingly.
6. x402 v2 headers are `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`, `PAYMENT-RESPONSE` (base64
   JSON). **An upstream NEAR `exact` scheme already exists** (delegate action, FullAccess payer,
   facilitator relayer; `@x402/near` 2.28.0) with `near:mainnet` / `near:testnet`.
7. The `near` CAIP-2 namespace is **not registered** (ChainAgnostic PR #203 open).
8. NEAR AI Cloud attestation **accepts a caller nonce** (32 bytes, 64 hex), bound into the last
   32 bytes of the TDX `report_data`; reports need an API key.
9. `SuccessReceiptId` never appears in the top-level `FinalExecutionStatus`. Top-level success
   does not prove the token receipt succeeded, and `ft_transfer_call` refunds can fake a naive
   event check.
10. Mainnet nearcore 2.13.4 rejects `tx_status` (`METHOD_NOT_FOUND`); use
    `EXPERIMENTAL_tx_status`.

## Errata for new_prompts

Each row is a statement in `new_prompts/02, 03, 04, 06, 08, 09` that this research contradicts,
qualifies, or cannot confirm. Accessed 2026-10-21. Finding types: **Contradicted**,
**Qualified** (true but incomplete in a way that affects the build), **Unconfirmed**.

| Prompt | Statement (quoted or close paraphrase) | Finding | Source |
|---|---|---|---|
| 03 | "Whether any NEAR scheme or network binding exists upstream. If none exists, define a NEAR profile" | **Contradicted.** `specs/schemes/exact/scheme_exact_near.md` exists upstream (added 2026-05-16, #1459), with `near:mainnet` / `near:testnet` and a reference package `@x402/near` 2.28.0. The tx-proof flow is a different `exact` method (client-submitted family), not a missing binding. | https://github.com/x402-foundation/x402/blob/main/specs/schemes/exact/scheme_exact_near.md |
| 03 | "No NEAR-native receiving or verifying side is known to this project. Re-verify before claiming novelty." | **Contradicted.** `@x402/near` ships facilitator verify and settle for NEAR. Novelty must be restated (tx-proof method, receipt-level verifier, policy-wallet payments). | https://registry.npmjs.org/@x402/near ; package README |
| 03 | "One x402 NEAR payer skill exists publicly; it is payer-side and Base-settled" | Confirmed (NearDeFi/agent-payments-skill settles on Base). | https://github.com/NearDeFi/agent-payments-skill |
| 03 | "secondary sources indicate PAYMENT-REQUIRED, PAYMENT-SIGNATURE and PAYMENT-RESPONSE in v2, carrying base64-encoded JSON" | Confirmed (v1 used `X-PAYMENT` / `X-PAYMENT-RESPONSE`). | specs/transports-v2/http.md |
| 03 | "whether a NEAR CAIP-2 namespace is registered (for example near:mainnet)" | **Not registered.** ChainAgnostic namespaces PR #203 is open with no reviews; the ids are de facto only. | https://github.com/ChainAgnostic/namespaces/pull/203 |
| 03 | Payment payload "`{network, intent_id, tx_hash, signer_id}` in the header the spec names" | **Qualified.** The header value must be a full v2 `PaymentPayload` (`x402Version`, `accepted`, `payload`); these fields go inside `payload`. The method must use `paymentFlow: "upfront"` and declare an `assetTransferMethod` in `extra`. | core spec 5.2; scheme_exact.md |
| 03 | "not final yet: `202`, or `402` with a `retry_after` hint. Choose one per spec" | **Resolved by the spec.** Under `upfront`, settle returns an error naming the unmet condition; `settlement_pending` (non-terminal, non-empty `transaction`) fits; the HTTP transport maps payment failures to `402`. No `202` in the spec. | core spec 9; transports-v2/http.md |
| 03 | Verifier condition 1: "final execution status is `SuccessValue`" (with condition 2's event check) | **Qualified.** Necessary, not sufficient. An `ft_transfer_call` to the gateway logs a matching event, then the reference `ft_resolve_transfer` refunds it with a reverse event (`memo: "refund"`). Require method `ft_transfer` (needs the `receipts` array) or net the gateway delta across events. | near-contract-standards `core_impl.rs` at near-sdk-v5.29.1 |
| 03 | "RPC `tx` / `EXPERIMENTAL_tx_status` ... `receipts_outcome` structure" | **Qualified.** Both return `receipts_outcome`; only `EXPERIMENTAL_tx_status` returns `receipts` (predecessor, method). `tx_status` (the docs' new name) is `METHOD_NOT_FOUND` on mainnet 2.13.4. | live RPC calls; nearcore CHANGELOG |
| 03 | "`parse_status` treats `SuccessReceiptId` as success at the final status level ... should be `SuccessValue` or `Failure`" | Confirmed: `FinalExecutionStatus` has no `SuccessReceiptId` variant. | nearcore 2.13.4 `views.rs` |
| 03 (P8) | "how a quote's recipient can be set to the gateway account with a binding memo or reference" / "bind ... by the quote's own reference field" | **Contradicted in part.** `recipient` can be the gateway, but `QuoteRequest` has no destination memo or reference field; the memo on 1Click's outgoing `ft_transfer` is undocumented. Bind by the unique `depositAddress` plus `destinationChainTxHashes`; `EXACT_OUTPUT` can fix the amount. | openapi.yaml 0.1.10 |
| 03 (P8) | "authentication (JWT or API key)" | **Qualified.** The partner JWT is sent as `X-API-Key` (recommended); `JWT-auth` bearer is "legacy". Unauthenticated quotes pay +0.25%. | openapi.yaml; authentication page |
| 03 (P8) | "the official Rust SDK name and version" | **Qualified.** `one-click-sdk-rs` 0.1.17, used as a git path dependency per the docs; the crates.io crate of the same name is owned by a personal account (provenance unconfirmed) and was last updated 2026-03-12. | sdk page; crates.io owners API |
| 03 / 09 | M-16 and P8 assume a test path | **Qualified.** "There is no testnet version of NEAR Intents"; every 1Click test is a real-value mainnet action. | sdk page |
| 03 | `clients/typescript` "using `near-api-js` (the official library, version pinned)" | **Unconfirmed.** The upstream NEAR x402 package depends on the modular `@near-js/*` packages (^2.5.1); the current status of `near-api-js` was not checked. | @x402/near package.json |
| 02 | "`cargo near build` (reproducible mode if available)" in CI | **Qualified.** Reproducible builds need Docker, a committed `Cargo.lock` and a clean tree; reproducible deploy and verification need the code pushed to `package.repository`, which is a human-only action (S3). | cargo-near README |
| 02 | "integration tests with `near-workspaces` sandbox" (implicitly on the builder's machine) | **Qualified.** The sandbox cannot run on native Windows. Use WSL2 or Linux CI. | near-sandbox-rs v0.3.16 `src/runner/mod.rs` |
| 02 | "the wasm target required by the current protocol" | Answered: `wasm32-unknown-unknown`; protocol 84+ accepts bulk-memory opcodes, so no rustc 1.86 pin with near-sdk 5.29.1. Avoid near-sdk 5.30.0-rc.1 on mainnet (PV 88 vs mainnet 86). | near-sdk Cargo.toml metadata; live `status` |
| 02 | Promise handling via "`env::promise_result`, or the `#[callback_result]` form" | **Qualified.** `env::promise_result` is deprecated in 5.29.1; use `promise_result_checked` or `#[callback_result]`. | near-sdk `env.rs` at v5.29.1 |
| 02 | `pay` does `ft_transfer` "with 1 yocto from the wallet's balance" | Confirmed, with a nuance: `one_yocto_on_promise` (live) lets a contract with zero balance attach 1 yocto to a promise; it does not relax the function-call-key rule. | nearcore 2.13.4 `logic.rs`; live protocol config |
| 02 / 09 | Gas budgets (allowance; "NEAR spent on gas: at most 0.10") | **Qualified.** Gas is purchased at 0.001 NEAR/Tgas and refunded at burn. A 30 Tgas call needs about 0.03 NEAR liquid; a 300 Tgas deploy about 0.3 NEAR, beyond the spent amount. Function-call-key allowances must cover the prepaid amount (Likely). | live protocol config; nearcore CHANGELOG 2.13.0; mainnet tx refund receipt |
| 04 / 06 | NEAR AI Cloud at `https://cloud-api.near.ai/v1`, API key, `verifiable` / `attestationSupported` flags, TDX plus NVIDIA | Confirmed. Note that 46 of 59 catalogue models are `external` (not attested); choose a `vllm` model. | docs.near.ai; live `/v1/model/list` |
| 06 | "direct completions mode: per-model subdomains under `completions.near.ai`, one TEE hop" | **Qualified.** It exists but is "Experimental" and "not recommended for ... production verification" (a report covers one instance; signatures can 404). | docs.near.ai direct-completions page |
| 06 | "TLS terminates inside the enclave" | **Unconfirmed as stated.** The docs provide an optional per-connection TLS binding to the gateway quote (`include_tls_fingerprint=true`, same connection) and state that browsers cannot verify it; they do not make a blanket termination claim. | docs.near.ai TLS page |
| 06 | "The model signing address should match the address that signs chat responses, and chat signatures can be fetched after completion" | **Qualified.** True, but the matching report must be fetched and verified **before** the completion; instances of one model share a signing key; only `provider_tee` signatures bind to the model, and the gateway re-signs streamed or aliased responses. | docs.near.ai response-signatures page |
| 06 | Research item 1: whether attestation endpoints accept a caller-supplied nonce | Answered: yes, `nonce` (32 random bytes, 64 hex) on `/v1/attestation/report`, bound into `report_data[32..64]`. It binds the report, not a completion. | gateway-attestation and quote-nonce-signer pages |
| 08 | "checks the key against `view_access_key_list` for the claimed account" | **Qualified.** NEP-413 requires a FullAccess key, so check `permission == "FullAccess"`. nearcore 2.14 paginates `view_access_key_list` (100-key cap) and changes missing-account errors; prefer `view_access_key`. | NEP-413; nearcore CHANGELOG 2.14.0 |
| 08 | "The caller signs `{method, path, body_hash, nonce, timestamp}`" | **Qualified.** NEP-413 signs `Payload{message, nonce[32], recipient, callbackUrl?}` with tag 2147484061; the request tuple must be serialized into `message`, and `recipient` fixed by the server. | NEP-413 |
| 09 | "Re-check the endpoint guidance (rate limits, the official deprecation notices for the public RPC, recommended providers)" | **No deprecation notice found** for `rpc.mainnet.near.org` on the providers page; only the archival endpoints are marked "Severely rate limited". The endpoint answered normally. | docs.near.org providers page; live calls |
| 09 | "The locked NEAR is recoverable on account deletion" | **Unconfirmed** directly (consistent with storage staking being part of the balance sent to the beneficiary on `DeleteAccount`). Verify on testnet before relying on it in the budget. | docs.near.org storage-staking page |

Statements outside the research scope (benchmark counts and splits in prompt 04, repository
internals, test counts) were not checked.

### Errata for docs/integrations/near.md (outside new_prompts, for the prompt 01 owner)

| Statement | Finding | Source |
|---|---|---|
| "EXPERIMENTAL_tx_status is a deprecated alias of tx_status" | True for nearcore 2.14 (testnet) and the docs; **false on mainnet 2.13.4**, where `tx_status` is `METHOD_NOT_FOUND`. | live RPC; nearcore CHANGELOG |
| Settlement asset row: bridged USDC.e (`a0b8...factory.bridge.near`) | Already superseded within the same file by the 2026-10-21 mainnet run (native USDC `17208628...`); `/v0/tokens` lists the native contract as the NEAR USDC. | live 1Click `/v0/tokens` |

## Not verified (open items)

- Building a contract with cargo-near 0.21.1 and rustc 1.98 on this host (inputs verified, build
  not run).
- Whether near-sdk 5.30.0-rc.1 wasm actually imports protocol-87/88 host functions.
- Function-call-key allowance accounting at the purchase price (measure on testnet).
- Logs of a failed receipt being retained in `outcome.logs` (fixture needed).
- The native USDC contract's own `ft_resolve_transfer` refund-event behaviour (the reference
  implementation is verified).
- The memo that 1Click puts on its destination `ft_transfer`, `EXACT_OUTPUT` refund rules, and
  the quote-signature key and algorithm.
- A live NEAR AI attestation report (needs an API key), the latency of attested models, and the
  NEAR AI SDK versions.
- Actual rate limits of the public RPC endpoints (not published).
