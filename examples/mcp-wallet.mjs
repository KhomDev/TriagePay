// A one-tool MCP server: the model may ask to pay, and the only call this
// builds is `pay` on a TriagePay policy wallet.
//
//   node examples/mcp-wallet.mjs
//
// WALLET and TOKEN come from the environment. This process does not read a
// key. It returns the call. Pass a `sign` function from your own program if
// you want the tool to broadcast; the stdio server below does not.

import readline from "node:readline";
import { policyPayCall } from "../packages/triagepay-sdk/src/index.js";

export const PROTOCOL = "2024-11-05";

export function toolDefinition() {
  return {
    name: "triagepay_pay",
    description: "Pay a payee through the TriagePay policy wallet. The wallet enforces the allowlist and the caps. This tool cannot withdraw or change policy.",
    inputSchema: {
      type: "object",
      additionalProperties: false,
      properties: {
        receiver_id: { type: "string", description: "NEAR account to pay." },
        amount: { type: "string", description: "Integer token amount in atomic units." },
        memo: { type: "string", description: "Optional memo, at most 256 bytes." },
      },
      required: ["receiver_id", "amount"],
    },
  };
}

export async function handleMessage(message, { wallet, token, sign } = {}) {
  if (!message || typeof message !== "object") return null;
  const { id, method, params } = message;
  if (method === "notifications/initialized" || method === "initialized") return null;
  if (method === "initialize") {
    return {
      jsonrpc: "2.0",
      id,
      result: {
        protocolVersion: PROTOCOL,
        capabilities: { tools: { listChanged: false } },
        serverInfo: { name: "triagepay-wallet", version: "0.1.0" },
      },
    };
  }
  if (method === "tools/list") {
    return { jsonrpc: "2.0", id, result: { tools: [toolDefinition()] } };
  }
  if (method === "tools/call") {
    const name = params && params.name;
    if (name !== "triagepay_pay") {
      return errorResult(id, "only triagepay_pay is available");
    }
    let call;
    try {
      const args = (params && params.arguments) || {};
      call = policyPayCall({
        wallet,
        token,
        receiverId: args.receiver_id,
        amount: args.amount,
        memo: args.memo,
      });
    } catch (err) {
      return errorResult(id, err.message);
    }
    let txHash = null;
    if (typeof sign === "function") txHash = await sign(call);
    const text = JSON.stringify({ call, tx_hash: txHash ? String(txHash) : null });
    return { jsonrpc: "2.0", id, result: { content: [{ type: "text", text }] } };
  }
  if (id === undefined) return null;
  return { jsonrpc: "2.0", id, error: { code: -32601, message: "method not found: " + method } };
}

function errorResult(id, text) {
  return { jsonrpc: "2.0", id, result: { isError: true, content: [{ type: "text", text }] } };
}

const runningAsCli = process.argv[1] && process.argv[1].endsWith("mcp-wallet.mjs");
if (runningAsCli) {
  const wallet = process.env.TRIAGEPAY_WALLET;
  const token = process.env.TRIAGEPAY_TOKEN;
  const rl = readline.createInterface({ input: process.stdin });
  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let message;
    try {
      message = JSON.parse(trimmed);
    } catch {
      return;
    }
    Promise.resolve(handleMessage(message, { wallet, token })).then((response) => {
      if (response) process.stdout.write(JSON.stringify(response) + "\n");
    });
  });
}
