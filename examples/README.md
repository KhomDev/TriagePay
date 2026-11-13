# Give an agent a TriagePay wallet

Two minutes, once the wallet exists:

1. The wallet account is already on mainnet. Creating one costs a 1 NEAR fee plus the refundable storage on the [/app](../demo/live/public/app/index.html) page.
2. I add one function-call key. It can call `pay` on that wallet and nothing else.
3. The agent pays with one call:

```js
import { payPerCall, createPolicyPayer } from "triagepay-sdk";

await payPerCall({
  url: "https://api.example/v2/scan",
  body: finding,
  payer: createPolicyPayer({ wallet, token, signerId, sign: (call) => near.signAndSend(call) }),
});
```

`sign` belongs to the agent. The SDK does not read a key.

## MCP

`examples/mcp-wallet.mjs` is a stdio MCP server with one tool, `triagepay_pay`. The tool builds `pay` and cannot withdraw or edit policy. The server I ship does not hold a key: it returns the call, and the process that owns the key signs it. Set `TRIAGEPAY_WALLET` and `TRIAGEPAY_TOKEN` before starting it.

```sh
TRIAGEPAY_WALLET=wallet.testnet TRIAGEPAY_TOKEN=usdc.testnet node examples/mcp-wallet.mjs
```

## An outside agent

`examples/pay-x402.mjs` is a small script that is not the TriagePay API. It receives a 402, pays through the policy wallet, and retries.

```sh
node examples/pay-x402.mjs --demo
```

`--demo` uses a local server and a stand-in signer. A real run passes the same `createPolicyPayer` with a `sign` that talks to the agent's NEAR account.
