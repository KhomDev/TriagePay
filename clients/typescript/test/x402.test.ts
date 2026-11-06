// Offline tests: an in-process mock x402 server, the pure payment
// planner, and CLI refusals that happen before any key or network use.

import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { buildPaymentPayload, decodeHeader, encodeHeader, payAndScan, selectRequirement, X402Error, type Requirement } from "../src/x402.ts";
import { assertNetworkAllowed, planPayment } from "../src/actions.ts";

const NETWORK = "near:testnet";

function requirement(memo: string): Requirement {
  return {
    scheme: "exact",
    network: NETWORK,
    amount: "1000000",
    asset: "usdc.mock.testnet",
    payTo: "gateway.mock.testnet",
    maxTimeoutSeconds: 900,
    extra: { assetTransferMethod: "near-tx-proof", paymentFlow: "upfront", memo, intentId: memo },
  };
}

async function mockServer(pendingPolls: number, failReason?: string) {
  const issued = new Map<string, Requirement>();
  const polls = new Map<string, number>();
  let charges = 0;
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const send = (status: number, payload: unknown, headers: Record<string, string> = {}) => {
        res.writeHead(status, { "content-type": "application/json", ...headers });
        res.end(JSON.stringify(payload));
      };
      const fail = (status: number, reason: string, tx = "") =>
        send(status, { error: { code: reason, message: reason } }, {
          "PAYMENT-RESPONSE": encodeHeader({ success: false, errorReason: reason, transaction: tx, network: NETWORK }),
          ...(reason === "settlement_pending" ? { "Retry-After": "0" } : {}),
        });
      const signature = req.headers["payment-signature"];
      if (!signature) {
        const memo = `aps2_mock_${issued.size + 1}`;
        issued.set(memo, requirement(memo));
        const required = { x402Version: 2, error: "PAYMENT-SIGNATURE header is required", resource: { url: "/v2/scan" }, accepts: [issued.get(memo)] };
        return send(402, { error: { code: "payment_required", message: "pay" } }, { "PAYMENT-REQUIRED": encodeHeader(required) });
      }
      const payload = decodeHeader(String(signature));
      const memo = payload.payload.intent_id;
      if (JSON.stringify(payload.accepted) !== JSON.stringify(issued.get(memo))) return fail(402, "invalid_payment_requirements");
      if (failReason) return fail(402, failReason, payload.payload.tx_hash);
      const n = polls.get(memo) ?? 0;
      if (n < pendingPolls) {
        polls.set(memo, n + 1);
        return fail(402, "settlement_pending", payload.payload.tx_hash);
      }
      charges++;
      send(200, { report: { decision: "supported", title: JSON.parse(body).title } }, {
        "PAYMENT-RESPONSE": encodeHeader({ success: true, transaction: payload.payload.tx_hash, network: NETWORK, payer: payload.payload.signer_id, amount: "1000000" }),
      });
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  return { url: `http://127.0.0.1:${port}`, close: () => server.close(), charges: () => charges };
}

test("one paid scan end to end against the mock provider server", async () => {
  const mock = await mockServer(2);
  try {
    const seen: Requirement[] = [];
    const result = await payAndScan(mock.url, { title: "finding" }, async (r) => {
      seen.push(r);
      return { tx_hash: `tx-${r.extra.memo}`, signer_id: "agent.mock.testnet" };
    }, { sleep: async () => {} });
    assert.equal(seen.length, 1);
    assert.equal(result.body.report.decision, "supported");
    assert.equal(result.paymentResponse.success, true);
    assert.equal(result.paymentResponse.transaction, `tx-${seen[0].extra.memo}`);
    assert.equal(result.attempts, 3);
    assert.equal(mock.charges(), 1);
  } finally {
    mock.close();
  }
});

test("typed failures carry the reason", async () => {
  const mock = await mockServer(0, "amount_mismatch");
  try {
    await assert.rejects(
      payAndScan(mock.url, { title: "x" }, async () => ({ tx_hash: "t", signer_id: "s" }), { sleep: async () => {} }),
      (error: X402Error) => error.reason === "amount_mismatch" && error.status === 402,
    );
  } finally {
    mock.close();
  }
});

test("requirement selection skips unknown flows and other networks", () => {
  const required = {
    x402Version: 2,
    accepts: [
      { scheme: "exact", network: NETWORK, extra: { assetTransferMethod: "near-tx-proof", paymentFlow: "escrow", memo: "a" } },
      requirement("b"),
    ],
  };
  assert.equal(selectRequirement(required).extra.memo, "b");
  assert.throws(() => selectRequirement(required, "near:mainnet"));
  assert.throws(() => selectRequirement({ x402Version: 1, accepts: [] }));
});

test("payment payload shape and header round trip", () => {
  const payload = buildPaymentPayload(requirement("m"), { tx_hash: "h", signer_id: "s" }, { url: "/v2/scan" });
  assert.deepEqual(payload.payload, { network: NETWORK, intent_id: "m", tx_hash: "h", signer_id: "s" });
  assert.deepEqual(decodeHeader(encodeHeader(payload)), payload);
  assert.throws(() => buildPaymentPayload(requirement("m"), { tx_hash: "", signer_id: "s" }));
});

test("direct and wallet payment plans", () => {
  const direct = planPayment(requirement("m1"), "direct");
  assert.equal(direct.receiverId, "usdc.mock.testnet");
  assert.equal(direct.methodName, "ft_transfer");
  assert.deepEqual(direct.args, { receiver_id: "gateway.mock.testnet", amount: "1000000", memo: "m1" });
  assert.equal(direct.deposit, 1n);
  const wallet = planPayment(requirement("m2"), "wallet", "wallet.mock.testnet");
  assert.equal(wallet.receiverId, "wallet.mock.testnet");
  assert.equal(wallet.methodName, "pay");
  assert.deepEqual(wallet.args, { token: "usdc.mock.testnet", receiver_id: "gateway.mock.testnet", amount: "1000000", memo: "m2" });
  assert.equal(wallet.deposit, 0n);
  assert.throws(() => planPayment(requirement("m3"), "wallet"));
  assert.throws(() => planPayment({ ...requirement("m4"), amount: "1.5" }, "direct"));
});

test("mainnet payments are refused without an explicit flag", () => {
  assert.throws(() => assertNetworkAllowed("near:mainnet", false), /allow-mainnet/);
  assert.doesNotThrow(() => assertNetworkAllowed("near:mainnet", true));
  assert.doesNotThrow(() => assertNetworkAllowed("near:testnet", false));
  assert.throws(() => assertNetworkAllowed("eip155:8453", true));
});

test("the CLI refuses a mainnet requirement before touching keys or the network", () => {
  const cli = join(dirname(fileURLToPath(import.meta.url)), "..", "src", "cli.ts");
  const mainnet = { ...requirement("m"), network: "near:mainnet" };
  const run = spawnSync(process.execPath, [cli, "pay", "--key-file", "does-not-exist.json"], {
    input: JSON.stringify(mainnet),
    encoding: "utf8",
  });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /allow-mainnet/);
  assert.equal(run.stdout, "");
});
