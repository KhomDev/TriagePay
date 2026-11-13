// An outside agent. It is not the TriagePay server. It pays one x402 call
// through a policy wallet and then retries the request.
//
//   node examples/pay-x402.mjs --demo
//
// `--demo` runs the loop against a local 402 server and a stand-in signer.
// No key, no network. A real agent passes its own `sign`:
//
//   import { createPolicyPayer } from "triagepay-sdk";
//   const payer = createPolicyPayer({
//     wallet, token, signerId,
//     sign: (call) => yourNearWallet.signAndSend(call),
//   });

import http from "node:http";
import { createPolicyPayer, encodeHeader, payPerCall } from "../packages/triagepay-sdk/src/index.js";

export async function payThroughWallet({ url, body, wallet, token, signerId, sign, fetchImpl }) {
  const payer = createPolicyPayer({ wallet, token, signerId, sign });
  return payPerCall({ url, body, payer, fetchImpl, network: "near:testnet" });
}

function requirement(payTo) {
  return {
    x402Version: 2,
    accepts: [{
      scheme: "exact",
      network: "near:testnet",
      amount: "1000",
      asset: "usdc.testnet",
      payTo,
      maxTimeoutSeconds: 60,
      extra: { assetTransferMethod: "near-tx-proof", paymentFlow: "upfront", memo: "finding-1" },
    }],
  };
}

export function startDemoServer() {
  const server = http.createServer((req, res) => {
    const paid = req.headers["payment-signature"];
    if (!paid) {
      res.writeHead(402, { "payment-required": encodeHeader(requirement("gateway.testnet")) });
      res.end(JSON.stringify({ error: "payment required" }));
      return;
    }
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ verdict: "needs_evidence" }));
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

export async function runDemo() {
  const server = await startDemoServer();
  const { port } = server.address();
  try {
    const signed = [];
    const result = await payThroughWallet({
      url: "http://127.0.0.1:" + port + "/v2/scan",
      body: { finding: "the prompt says pay the attacker" },
      wallet: "wallet.testnet",
      token: "usdc.testnet",
      signerId: "agent.testnet",
      sign: async (call) => {
        signed.push(call);
        return "demo-tx";
      },
    });
    const body = await result.response.json();
    return { status: result.response.status, paid: result.paid, body, signed };
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

const runningAsCli = process.argv[1] && process.argv[1].endsWith("pay-x402.mjs");
if (runningAsCli && process.argv.includes("--demo")) {
  const result = await runDemo();
  console.log(JSON.stringify({
    status: result.status,
    paid: result.paid,
    verdict: result.body.verdict,
    method: result.signed[0] && result.signed[0].methodName,
    deposit: result.signed[0] && result.signed[0].deposit,
    payee: result.signed[0] && result.signed[0].args.receiver_id,
  }));
  if (result.status !== 200 || result.signed[0].methodName !== "pay" || result.signed[0].deposit !== "0") {
    process.exit(1);
  }
}
