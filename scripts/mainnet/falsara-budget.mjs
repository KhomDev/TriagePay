// Give the factory-created mainnet wallet a USDC budget for Falsara checks.
//
//   APS_MAINNET_ENV=/path/outside/the/repo.env node scripts/mainnet/falsara-budget.mjs [--dry-run] [--out FILE]
//
// Two owner transactions, each exactly 1 yoctoNEAR attached:
//   1. USDC ft_transfer_call: owner -> wallet, FALSARA_FUND_ATOMIC (default
//      1500000 = 1.50 USDC). The wallet's ft_on_transfer accepts allowlisted
//      tokens up to its beta deposit cap.
//   2. set_token_limits for the agent: per call = one Falsara check
//      (250000 = 0.25 USDC), per UTC day = FALSARA_DAILY_ATOMIC (default
//      1500000), so the contract itself limits the agent to six checks a day.
// The key file is read from APS_MAINNET_ENV and never printed.

import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..");
const RPC = "https://free.rpc.fastnear.com";
const USDC = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
const WALLET = process.env.APS_MAINNET_WALLET || "e2e044e80.triagepay.hlpok1lad.tg";
const AGENT = process.env.APS_MAINNET_AGENT || "tp-e2e-agent.hlpok1lad.tg";
const FUND = BigInt(process.env.FALSARA_FUND_ATOMIC || "1500000");
const PER_CALL = BigInt(process.env.FALSARA_PER_CALL_ATOMIC || "250000");
const DAILY = BigInt(process.env.FALSARA_DAILY_ATOMIC || "1500000");
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const outAt = args.indexOf("--out");
const out = outAt >= 0 ? args[outAt + 1] : null;

async function view(contract, method, a) {
  const response = await fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "query", params: { request_type: "call_function", finality: "final", account_id: contract, method_name: method, args_base64: Buffer.from(JSON.stringify(a)).toString("base64") } }),
  });
  const body = await response.json();
  if (body.error || body.result.error) throw new Error(`view ${method} failed`);
  return JSON.parse(Buffer.from(body.result.result).toString("utf8"));
}

function owner() {
  const file = process.env.APS_MAINNET_ENV;
  if (!file) throw new Error("set APS_MAINNET_ENV to the owner's env file (outside the repository)");
  const rel = resolve(file).startsWith(REPO);
  if (rel) throw new Error("APS_MAINNET_ENV must be outside the repository");
  // Same parsing as scripts/mainnet/mainnet-e2e.mjs: optional quotes, and
  // the ed25519: prefix is added when the file stores the bare key.
  const vars = {};
  for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
    const m = line.match(/^\s*(NEAR_ACCOUNT_ID|NEAR_PRIVATE_KEY|NEAR_NETWORK)\s*=\s*"?([^"\s]+)"?\s*$/);
    if (m) vars[m[1]] = m[2];
  }
  if (vars.NEAR_NETWORK && vars.NEAR_NETWORK !== "mainnet") throw new Error("the env file is not for mainnet");
  if (!vars.NEAR_ACCOUNT_ID || !vars.NEAR_PRIVATE_KEY) throw new Error("the env file lacks NEAR_ACCOUNT_ID or NEAR_PRIVATE_KEY");
  const secret = vars.NEAR_PRIVATE_KEY.startsWith("ed25519:") ? vars.NEAR_PRIVATE_KEY : `ed25519:${vars.NEAR_PRIVATE_KEY}`;
  return { accountId: vars.NEAR_ACCOUNT_ID, secret };
}

const who = owner();
const config = await view(WALLET, "get_config", {});
if (config.owner !== who.accountId) throw new Error(`the env file signs for ${who.accountId}, the wallet owner is ${config.owner}`);
const before = {
  ownerUsdc: await view(USDC, "ft_balance_of", { account_id: who.accountId }),
  walletUsdc: await view(USDC, "ft_balance_of", { account_id: WALLET }),
  agent: await view(WALLET, "get_agent", { agent_id: AGENT }),
};
const plan = {
  owner: who.accountId,
  wallet: WALLET,
  agent: AGENT,
  fundAtomic: FUND.toString(),
  perCallAtomic: PER_CALL.toString(),
  dailyAtomic: DAILY.toString(),
  before,
};
if (BigInt(before.ownerUsdc) < FUND) throw new Error(`the owner holds ${before.ownerUsdc} atomic USDC, less than ${FUND}`);
console.log(JSON.stringify({ dryRun, ...plan }, null, 2));
if (dryRun) process.exit(0);

const require = createRequire(join(REPO, "clients", "typescript", "package.json"));
const n = await import(pathToFileURL(require.resolve("near-api-js")).href);
const account = new n.Account(who.accountId, new n.JsonRpcProvider({ url: RPC }), who.secret);
const gas = (tgas) => BigInt(tgas) * 1_000_000_000_000n;
const summarize = (result) => ({
  tx: result.transaction.hash,
  ok: Boolean(result.status && result.status.SuccessValue !== undefined),
  error: (JSON.stringify(result).match(/ERR_[A-Z_]+/) || [null])[0],
});

const fund = summarize(await account.signAndSendTransaction({
  receiverId: USDC,
  actions: [n.actions.functionCall("ft_transfer_call", { receiver_id: WALLET, amount: FUND.toString(), msg: "" }, gas(100), 1n)],
  waitUntil: "FINAL",
  throwOnFailure: false,
}));
console.log("fund", fund);
const limits = summarize(await account.signAndSendTransaction({
  receiverId: WALLET,
  actions: [n.actions.functionCall("set_token_limits", { agent_id: AGENT, token: USDC, per_call_max: PER_CALL.toString(), daily_max: DAILY.toString() }, gas(30), 1n)],
  waitUntil: "FINAL",
  throwOnFailure: false,
}));
console.log("limits", limits);
const after = {
  ownerUsdc: await view(USDC, "ft_balance_of", { account_id: who.accountId }),
  walletUsdc: await view(USDC, "ft_balance_of", { account_id: WALLET }),
  agent: await view(WALLET, "get_agent", { agent_id: AGENT }),
};
const record = { ...plan, at: new Date().toISOString(), fund, limits, after };
if (out) writeFileSync(out, JSON.stringify(record, null, 2) + "\n");
console.log(JSON.stringify({ after }, null, 2));
process.exit(fund.ok && limits.ok ? 0 : 1);
