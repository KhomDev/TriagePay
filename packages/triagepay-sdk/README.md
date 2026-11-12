# triagepay-sdk

One call wraps an agent's payment in the TriagePay policy wallet. The key
that signs can call only `pay`. The wallet caps the call, caps the day, and
allowlists the payee.

```js
import { payPerCall, createPolicyPayer } from "triagepay-sdk";

await payPerCall({
  url: "https://api.example/v2/scan",
  body: finding,
  payer: createPolicyPayer({
    wallet: "wallet.testnet",
    signerId: "agent.testnet",
    sign: (call) => near.signAndSend(call),
  }),
});
```

`sign(call)` is yours. `call` is `{ receiverId, methodName: "pay", args, gas, deposit: "0" }`.
This package does not read a secret and does not import `near-api-js`.

A NEAR AI tool that can do the same call, and nothing else:

```js
import { nearAiPaymentTool } from "triagepay-sdk";

const tool = nearAiPaymentTool({
  wallet: "wallet.testnet",
  token: "wrap.testnet",
  sign: (call) => near.signAndSend(call),
});
```

Register `tool.function` with the model and run `tool.execute` only after
the model asks for it. The wallet still refuses a payee that is not allowed.

## Chain Signatures

`ecdsaSignRequest` and `erc20TransferCalldata` build a NEAR `sign` request
for an ERC-20 `transfer` on Base Sepolia. They do not submit it. The design
and the gap are in [docs/contracts/chain-signatures.md](../../docs/contracts/chain-signatures.md).

## Tests

```sh
cd packages/triagepay-sdk
npm test
```

No install step: the package has no dependencies.
