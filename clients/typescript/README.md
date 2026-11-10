# aps-x402 (TypeScript reference payer)

A reference payer and client for the TriagePay x402 near-tx-proof
method ([`docs/protocols/x402-near.md`](../../docs/protocols/x402-near.md)),
built on `near-api-js` 7.3.1 (pinned exactly). It runs on Node 22.18+ with
built-in TypeScript type stripping; there is no build step.

Two ways to pay a requirement:

| Mode | Call | Key needed |
|---|---|---|
| `direct` | `ft_transfer(receiver_id = payTo, amount, memo)` on the asset, 1 yoctoNEAR | FullAccess key of the payer |
| `wallet` | `pay(token = asset, receiver_id = payTo, amount, memo)` on a policy wallet, zero deposit | function-call key limited to the wallet's `pay` |

The key file is JSON `{"account_id": "...", "secret_key": "ed25519:..."}`,
kept outside any repository. The CLI never prints it. Mainnet payments are
refused unless `--allow-mainnet` is passed.

## Pay for a tool call in 10 lines

```ts
import { payAndScan } from "./src/x402.ts";
import { nearPayer } from "./src/payer.ts";

const pay = nearPayer({ mode: "direct", keyFile: "/path/outside/repo/agent.json" });
const result = await payAndScan("http://127.0.0.1:8080", { title: "...", mechanism: "..." }, pay);
console.log(result.body.report.decision, result.paymentResponse.transaction);
```

## CLI

```sh
npm install
# Pay one requirement read from stdin; prints {"tx_hash","signer_id"} (used by clients/python shell_payer):
node src/cli.ts pay --mode direct --key-file /path/outside/repo/agent.json < requirement.json
node src/cli.ts pay --mode wallet --wallet wallet.example.testnet --key-file /path/outside/repo/agent.json < requirement.json
# The full round trip against a running API:
node src/cli.ts scan --url http://127.0.0.1:8080 --envelope envelope.json --mode direct --key-file /path/outside/repo/agent.json
```

Default RPC endpoints are FASTNEAR's free tier (`https://test.rpc.fastnear.com`,
`https://free.rpc.fastnear.com`); the official `rpc.*.near.org` endpoints
are deprecated. Override with `--rpc-url`.

## Tests

Offline: an in-process mock x402 server, the pure payment planner, and the
CLI's mainnet refusal.

```sh
npm test
```
