# x402 on NEAR mainnet, 2026-11-14

One paid x402 call on NEAR mainnet, settled in native USDC through the factory-created policy
wallet, and verified by the server from the USDC contract's own NEP-141 event. Recorded by
`scripts/mainnet/x402-mainnet.mjs`; every step is in [EVIDENCE.json](EVIDENCE.json).

The earlier mainnet wallet (`tp-wallet.hlpok1lad.tg`) holds `wrap.near`, which emits no NEP-141
events, so the receipt check could not run there. Native USDC
(`17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`) does emit them.

## Setup

| | |
|---|---|
| Server | `aps-api` from `main`, run locally with `APS_PROVIDER=near`, `NEAR_NETWORK=mainnet`, `NEAR_GATEWAY_ACCOUNT=gateway.hlpok1lad.tg`, `NEAR_ASSET_CONTRACT=<native USDC>`, `APS_X402_PRICE_ATOMIC=1000` (0.001 USDC per scan), RPC `https://free.rpc.fastnear.com` |
| Wallet | [`e2e044e80.triagepay.hlpok1lad.tg`](https://nearblocks.io/address/e2e044e80.triagepay.hlpok1lad.tg), created by the factory `triagepay.hlpok1lad.tg`. USDC budget for the agent: 1000 per call, 2000 per UTC day. Payee allowlist `[gateway.hlpok1lad.tg]` |
| Agent | `tp-e2e-agent.hlpok1lad.tg`, signing with its function-call key that can call only `pay` on the wallet |
| Finding | `agents/demo/findings/2-rounding-drain-with-evidence.json` |

## Steps

| Step | What happened | Result |
|---|---|---|
| Quote | `POST /v2/scan` without payment: 402 with `near:mainnet`, native USDC, `payTo` the gateway, 1000 atomic, a server-issued memo. The script checked network, asset, payee, allowlist, per-call cap, today's budget and wallet balance before signing | pass |
| Pay | The agent's pay-only key called `pay` on the wallet with that memo. Events: `aps_policy_wallet:pay_requested`, `nep141:ft_transfer` (wallet → gateway, 1000, the memo), `aps_policy_wallet:pay_settled`. [CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG](https://nearblocks.io/txns/CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG) | pass |
| Paid scan | The same request with `PAYMENT-SIGNATURE` naming the transaction: the server read the finalized receipt, matched the USDC contract's own `ft_transfer` event (executor, recipient, amount, memo) and answered 200. Verdict `supported`; `PAYMENT-RESPONSE` names payer `e2e044e80.triagepay.hlpok1lad.tg` and report hash `3e54d0350e579c51419e1cbe94a85835ddaa52921187bf5acd935d9c46a977d7` | pass |
| Replay | The same proof and the same body: 200 with the stored response, no second charge | pass |
| Reuse | The same proof with a different finding: 402 `proof_already_consumed` | pass |

Balances (atomic USDC): wallet 1000 → 0, gateway 101000 → 102000. Gas came from the agent key's
allowance. The replay and reuse checks sign nothing.

## Re-run

```sh
APS_PORT=18402 APS_PROVIDER=near NEAR_NETWORK=mainnet \
  NEAR_GATEWAY_ACCOUNT=gateway.hlpok1lad.tg \
  NEAR_ASSET_CONTRACT=17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1 \
  APS_X402_PRICE_ATOMIC=1000 cargo run -p aps-api --bin aps-api
node scripts/mainnet/x402-mainnet.mjs --api http://127.0.0.1:18402 --out-dir DIR --dry-run
node scripts/mainnet/x402-mainnet.mjs --api http://127.0.0.1:18402 --out-dir DIR --key /path/outside/the/repo/agent_pay.json
```

`--dry-run` reads only the quote and the wallet views. A paid run needs USDC in the wallet and
budget left for the day.

## What this does not show

The server ran on a local machine, not as a hosted service. The paid API still keeps its ledger
in memory, serves one development caller and binds loopback; see the README's Status table.
