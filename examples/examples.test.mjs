import test from "node:test";
import assert from "node:assert/strict";
import { handleMessage } from "./mcp-wallet.mjs";
import { runDemo } from "./pay-x402.mjs";

test("the MCP server offers one tool and the tool only builds pay", async () => {
  const init = await handleMessage({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} });
  assert.equal(init.result.serverInfo.name, "triagepay-wallet");
  assert.equal(init.result.protocolVersion, "2024-11-05");
  const list = await handleMessage({ jsonrpc: "2.0", id: 2, method: "tools/list" });
  assert.deepEqual(list.result.tools.map((tool) => tool.name), ["triagepay_pay"]);
  const signed = [];
  const call = await handleMessage(
    { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "triagepay_pay", arguments: { receiver_id: "gateway.testnet", amount: "1000", memo: "job" } } },
    { wallet: "wallet.testnet", token: "usdc.testnet", sign: async (built) => { signed.push(built); return "tx-1"; } },
  );
  const payload = JSON.parse(call.result.content[0].text);
  assert.equal(payload.tx_hash, "tx-1");
  assert.equal(payload.call.methodName, "pay");
  assert.equal(payload.call.deposit, "0");
  assert.equal(signed[0].args.receiver_id, "gateway.testnet");
  const refused = await handleMessage(
    { jsonrpc: "2.0", id: 4, method: "tools/call", params: { name: "withdraw", arguments: {} } },
    { wallet: "wallet.testnet", token: "usdc.testnet" },
  );
  assert.equal(refused.result.isError, true);
});

test("an outside agent pays a 402 through the policy wallet", async () => {
  const result = await runDemo();
  assert.equal(result.status, 200);
  assert.equal(result.paid, true);
  assert.equal(result.body.verdict, "needs_evidence");
  assert.equal(result.signed.length, 1);
  assert.equal(result.signed[0].receiverId, "wallet.testnet");
  assert.equal(result.signed[0].args.receiver_id, "gateway.testnet");
  assert.equal(result.signed[0].args.memo, "finding-1");
  assert.equal(result.signed[0].deposit, "0");
});
