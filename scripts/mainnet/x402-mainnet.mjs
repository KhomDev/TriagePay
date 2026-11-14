// One x402 round trip on NEAR mainnet, paid through a policy wallet.
//
//   node scripts/mainnet/x402-mainnet.mjs --api http://127.0.0.1:PORT \
//     --key /path/outside/the/repo/agent_pay.json --out-dir DIR [--dry-run]
//
// The API must run with the near provider on mainnet, for example:
//   APS_PROVIDER=near NEAR_NETWORK=mainnet NEAR_GATEWAY_ACCOUNT=<gateway>
//   NEAR_ASSET_CONTRACT=<native USDC> APS_X402_PRICE_ATOMIC=1000 aps-api
//
// What it does:
//   1. POST /v2/scan without payment and read the 402 requirement.
//   2. Refuse unless the requirement is near:mainnet, native USDC, pays the
//      allowlisted gateway, and fits the agent's per-call budget.
//   3. The agent's pay-only key calls `pay` on the wallet with the
//      requirement's memo. The wallet sends the USDC with ft_transfer.
//   4. Present the transaction hash in PAYMENT-SIGNATURE and retry until the
//      server answers 200. The server reads the USDC contract's own NEP-141
//      event from the finalized receipt.
//   5. Two checks that sign nothing: the same proof and body replay the
//      stored response, and the same proof with another body is refused.
//
// --dry-run stops after step 2 and does not read the key. The key file is
// {account_id, secret_key}; the secret is never printed or written.

import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..", "..");
const RPC = process.env.APS_MAINNET_RPC || "https://free.rpc.fastnear.com";
const EXPLORER = "https://nearblocks.io/txns/";
const USDC = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
const WALLET = process.env.APS_MAINNET_WALLET || "e2e044e80.triagepay.hlpok1lad.tg";
const GATEWAY = process.env.APS_MAINNET_GATEWAY || "gateway.hlpok1lad.tg";
const FINDING = path.join(REPO, "agents", "demo", "findings", "2-rounding-drain-with-evidence.json");
const OTHER_FINDING = path.join(REPO, "agents", "demo", "findings", "1-rounding-drain-placeholder-evidence.json");

const args = process.argv.slice(2);
const arg = (name) => {
  const i = args.indexOf("--" + name);
  return i >= 0 ? args[i + 1] : null;
};
const fail = (message) => {
  console.error("x402-mainnet: " + message);
  process.exit(1);
};
const api = arg("api");
const outDir = arg("out-dir");
const keyPath = arg("key");
const dryRun = args.includes("--dry-run");
if (!api) fail("--api is required");
if (!outDir) fail("--out-dir is required");
if (!dryRun && !keyPath) fail("--key is required (or pass --dry-run)");
if (keyPath) {
  const rel = path.relative(REPO, path.resolve(keyPath));
  if (rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel))) fail("--key must be outside the repository");
}

const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64");
const unb64 = (raw) => JSON.parse(Buffer.from(raw, "base64").toString("utf8"));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const redact = (s) => String(s).replace(/ed25519:[1-9A-HJ-NP-Za-km-z]{30,}/g, "ed25519:<key>");

async function rpc(method, params) {
  const response = await fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "x402-mainnet", method, params }),
  });
  const body = await response.json();
  if (body.error) throw new Error(`${method}: ${JSON.stringify(body.error).slice(0, 200)}`);
  return body.result;
}
async function view(contract, method, a) {
  const result = await rpc("query", {
    request_type: "call_function",
    finality: "final",
    account_id: contract,
    method_name: method,
    args_base64: Buffer.from(JSON.stringify(a)).toString("base64"),
  });
  return JSON.parse(Buffer.from(result.result).toString("utf8"));
}

const evidence = {
  run: "TriagePay x402 on NEAR mainnet",
  network: "near:mainnet",
  rpc: RPC,
  started_at: new Date().toISOString(),
  wallet: WALLET,
  gateway: GATEWAY,
  asset: USDC,
  steps: [],
};
const record = (step) => {
  evidence.steps.push({ at: new Date().toISOString(), ...step });
  console.log(`${step.pass === false ? "FAIL" : "ok  "} ${step.step}: ${step.summary}`);
  return step;
};

async function post(envelope, signature) {
  const headers = { "content-type": "application/json" };
  if (signature) headers["PAYMENT-SIGNATURE"] = signature;
  const response = await fetch(api + "/v2/scan", { method: "POST", headers, body: JSON.stringify(envelope) });
  const text = await response.text();
  let body = null;
  try {
    body = JSON.parse(text);
  } catch {
    body = null;
  }
  const required = response.headers.get("payment-required");
  const paymentResponse = response.headers.get("payment-response");
  return {
    status: response.status,
    body,
    required: required ? unb64(required) : null,
    payment: paymentResponse ? unb64(paymentResponse) : null,
  };
}

const envelope = JSON.parse(fs.readFileSync(FINDING, "utf8"));
const other = JSON.parse(fs.readFileSync(OTHER_FINDING, "utf8"));

// ----- 1. quote ----------------------------------------------------------------
const quote = await post(envelope);
if (quote.status !== 402 || !quote.required) fail(`expected a 402 with PAYMENT-REQUIRED, got ${quote.status}`);
const requirement = quote.required.accepts[0];

// ----- 2. check before signing ---------------------------------------------------
const config = await view(WALLET, "get_config", {});
const signerId = keyPath ? JSON.parse(fs.readFileSync(keyPath, "utf8")).account_id : (process.env.APS_MAINNET_AGENT || "tp-e2e-agent.hlpok1lad.tg");
const agent = await view(WALLET, "get_agent", { agent_id: signerId });
const budget = (agent.budgets || []).find((row) => row.token === USDC) || null;
const walletUsdc = BigInt(await view(USDC, "ft_balance_of", { account_id: WALLET }));
const problems = [];
if (requirement.network !== "near:mainnet") problems.push(`network ${requirement.network}`);
if (requirement.asset !== USDC) problems.push(`asset ${requirement.asset}`);
if (requirement.payTo !== GATEWAY) problems.push(`payTo ${requirement.payTo}`);
if (!agent.active) problems.push("agent inactive");
if (!(agent.payees || []).includes(requirement.payTo)) problems.push("payTo not on the wallet allowlist");
if (!budget) problems.push("no USDC budget for the agent");
if (budget && BigInt(requirement.amount) > BigInt(budget.per_call_max)) problems.push("amount above the per-call cap");
if (budget && BigInt(requirement.amount) > BigInt(budget.remaining)) problems.push("amount above today's remaining budget");
if (BigInt(requirement.amount) > walletUsdc) problems.push("wallet balance below the amount");
if (config.paused) problems.push("wallet paused");
record({
  step: "quote",
  pass: problems.length === 0,
  requirement,
  wallet_usdc_before: walletUsdc.toString(),
  agent_budget: budget,
  summary: problems.length
    ? `refusing to pay: ${problems.join("; ")}`
    : `402: ${requirement.amount} atomic USDC to ${requirement.payTo}, memo ${requirement.extra.memo}; wallet holds ${walletUsdc}, budget remaining ${budget.remaining}`,
});
if (problems.length || dryRun) {
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, dryRun ? "dry-run.json" : "EVIDENCE.json"), JSON.stringify(evidence, null, 2) + "\n");
  process.exit(problems.length ? 1 : 0);
}

// ----- 3. pay through the wallet --------------------------------------------------
const requireFromClient = createRequire(path.join(REPO, "clients", "typescript", "package.json"));
const n = await import(pathToFileURL(requireFromClient.resolve("near-api-js")).href);
const key = JSON.parse(fs.readFileSync(keyPath, "utf8"));
const provider = new n.JsonRpcProvider({ url: RPC });
const account = new n.Account(key.account_id, provider, key.secret_key);
const gatewayBefore = BigInt(await view(USDC, "ft_balance_of", { account_id: GATEWAY }));
let paid;
try {
  paid = await account.signAndSendTransaction({
    receiverId: WALLET,
    actions: [n.actions.functionCall("pay", { token: USDC, receiver_id: requirement.payTo, amount: requirement.amount, memo: requirement.extra.memo }, 50_000_000_000_000n, 0n)],
    waitUntil: "FINAL",
    throwOnFailure: false,
  });
} catch (error) {
  fail("pay was not sent: " + redact(error && error.message ? error.message : error));
}
const events = (paid.receipts_outcome || [])
  .flatMap((o) => o.outcome.logs)
  .filter((line) => line.startsWith("EVENT_JSON:"))
  .map((line) => {
    try {
      const e = JSON.parse(line.slice(11));
      return { standard: e.standard, event: e.event, data: e.data };
    } catch {
      return { standard: "unparsed" };
    }
  });
const txHash = paid.transaction.hash;
const settled = paid.status && paid.status.SuccessValue !== undefined
  && events.some((e) => e.standard === "aps_policy_wallet" && e.event === "pay_settled");
const nep141 = events.find((e) => e.standard === "nep141" && e.event === "ft_transfer");
record({
  step: "wallet_pay",
  pass: settled && Boolean(nep141),
  tx_hash: txHash,
  explorer: EXPLORER + txHash,
  events: events.map((e) => `${e.standard}:${e.event}`),
  nep141_transfer: nep141 ? nep141.data : null,
  summary: settled
    ? `the agent's pay-only key called pay; the wallet sent ${requirement.amount} atomic USDC to ${requirement.payTo} (${EXPLORER}${txHash})`
    : `pay did not settle (${EXPLORER}${txHash})`,
});
if (!settled) {
  fs.writeFileSync(path.join(outDir, "EVIDENCE.json"), JSON.stringify(evidence, null, 2) + "\n");
  process.exit(1);
}

// ----- 4. present the proof ---------------------------------------------------------
const signature = b64({
  x402Version: 2,
  resource: { url: "/v2/scan" },
  accepted: requirement,
  payload: { network: requirement.network, intent_id: requirement.extra.memo, tx_hash: txHash, signer_id: key.account_id },
});
let result = null;
for (let attempt = 0; attempt < 40; attempt++) {
  result = await post(envelope, signature);
  if (result.status === 200) break;
  const reason = result.body && result.body.error && result.body.error.code;
  const headerReason = result.payment && result.payment.errorReason;
  if (reason !== "settlement_pending" && headerReason !== "settlement_pending") break;
  await sleep(3000);
}
const report = result && result.body && result.body.report;
record({
  step: "paid_scan",
  pass: result.status === 200 && Boolean(report),
  http_status: result.status,
  payment_response: result.payment,
  decision: report ? report.decision : null,
  content_hash: report ? report.content_hash : null,
  settlement: result.body && result.body.settlement ? result.body.settlement : null,
  summary: result.status === 200
    ? `the server verified the USDC contract's nep141 ft_transfer event and answered 200: verdict ${report.decision}, payer ${result.payment && result.payment.payer}`
    : `the server answered ${result.status}: ${JSON.stringify(result.body).slice(0, 200)}`,
});

// ----- 5. replay and reuse (no signature) ------------------------------------------
const replay = await post(envelope, signature);
record({
  step: "replay_same_request",
  pass: replay.status === 200 && replay.body && report && replay.body.report.content_hash === report.content_hash,
  http_status: replay.status,
  summary: replay.status === 200 ? "the same proof and body replay the stored response; nothing is charged again" : `replay answered ${replay.status}`,
});
const reuse = await post(other, signature);
const reuseReason = (reuse.body && reuse.body.error && reuse.body.error.code) || (reuse.payment && reuse.payment.errorReason);
record({
  step: "reuse_other_request",
  pass: reuse.status !== 200,
  http_status: reuse.status,
  reason: reuseReason || null,
  summary: reuse.status !== 200 ? `the same proof with another finding is refused (${reuse.status} ${reuseReason})` : "the same proof bought a second scan",
});

const gatewayAfter = BigInt(await view(USDC, "ft_balance_of", { account_id: GATEWAY }));
const walletAfter = BigInt(await view(USDC, "ft_balance_of", { account_id: WALLET }));
evidence.balances = {
  wallet_usdc: { before: walletUsdc.toString(), after: walletAfter.toString() },
  gateway_usdc: { before: gatewayBefore.toString(), after: gatewayAfter.toString() },
};
evidence.finished_at = new Date().toISOString();
evidence.pass = evidence.steps.every((step) => step.pass !== false);
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "EVIDENCE.json"), JSON.stringify(evidence, null, 2) + "\n");
console.log(evidence.pass ? "x402 on mainnet: all checks passed" : "x402 on mainnet: a check failed");
process.exit(evidence.pass ? 0 : 1);
