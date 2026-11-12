// Agent demo on NEAR testnet (zero value, throwaway accounts).
//
// An agent pays TriagePay per call through x402, from a spend-policy wallet,
// with a function-call key that can only call `pay` on that wallet. The run
// shows what the wallet lets the key do and what it refuses:
//
//   node agents/demo/demo.mjs setup  --out-dir DIR --funder-key FILE
//   node agents/demo/demo.mjs run    --out-dir DIR --api http://127.0.0.1:PORT [--pause]
//   node agents/demo/demo.mjs direct --out-dir DIR --api http://127.0.0.1:PORT
//   node agents/demo/demo.mjs revoke --out-dir DIR --api http://127.0.0.1:PORT
//
// `setup` creates the accounts, deploys the test token and the wallet, funds
// the wallet, sets the agent's policy and adds the agent's function-call
// key. It prints the two values the API needs (gateway account and asset).
// `run` is the agent: it scans four findings and then tries three things a
// compromised agent would try. `direct` pays one call with a plain
// `ft_transfer` from a full-access account (the wallet is optional for the
// server) and replays the proof. `revoke` is the owner cutting the agent off.
//
// The agent here has no language model. Its "decisions" are a fixed loop
// plus one deliberately naive behaviour (following a payment instruction
// found in a finding), so that the wallet's refusals can be shown. The
// point of the demo is what the chain enforces whatever the agent decides.
//
// Keys are written to DIR/keys (DIR must be outside the repository) and are
// never printed. Evidence goes to DIR/agent-demo-EVIDENCE.json and a
// readable transcript to DIR/transcript.md. Testnet only: the script checks
// the endpoint's chain id before signing anything.
//
// Requires near-api-js 7.3.1: `npm ci --prefix clients/typescript`.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..", "..");
const requireFromClient = createRequire(path.join(REPO, "clients", "typescript", "package.json"));
const n = await import(pathToFileURL(requireFromClient.resolve("near-api-js")).href);

const RPC = process.env.APS_TESTNET_RPC || "https://test.rpc.fastnear.com";
const EXPLORER = "https://testnet.nearblocks.io/txns/";
const REGISTRAR = "testnet";
const PRICE = 1_000_000n; // one call: 1.000000 of the 6-decimal test token
const CALLS_PER_DAY = 4n;
const FUNDING = 10_000_000n; // the wallet's working balance: 10 tokens
const SUPPLY = 1_000_000_000_000n;
const ALLOWANCE = "0.5"; // NEAR of gas the agent key may burn

const args = process.argv.slice(2);
const command = args[0];
const flag = (name) => {
  const i = args.indexOf("--" + name);
  return i >= 0 ? args[i + 1] : undefined;
};
const outDir = flag("out-dir") && path.resolve(flag("out-dir"));
if (!outDir) fail("--out-dir is required");
const rel = path.relative(REPO, outDir);
if (rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel))) fail("--out-dir must be outside the repository");
const KEYS = path.join(outDir, "keys");
const EVIDENCE = path.join(outDir, "agent-demo-EVIDENCE.json");
const TRANSCRIPT = path.join(outDir, "transcript.md");

const T = (tgas) => BigInt(tgas) * 1_000_000_000_000n;
const NEAR = (s) => BigInt(n.parseNearAmount(s));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const provider = new n.JsonRpcProvider({ url: RPC });
const fc = (method, a, tgas, deposit) => n.actions.functionCall(method, a, T(tgas), deposit || 0n);
const units = (atomic) => (Number(atomic) / 1e6).toFixed(6);

function fail(message) {
  console.error("STOP: " + message);
  process.exit(2);
}

// ----- evidence and transcript ------------------------------------------------
let ev = fs.existsSync(EVIDENCE)
  ? JSON.parse(fs.readFileSync(EVIDENCE, "utf8"))
  : { run: "TriagePay agent demo", network: "near:testnet", rpc: RPC, explorer: EXPLORER, value: "zero (testnet tokens only)", started_at: new Date().toISOString(), tooling: "near-api-js 7.3.1, Node " + process.version, accounts: {}, policy: {}, steps: [] };
function save() {
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(EVIDENCE, JSON.stringify(ev, null, 2) + "\n");
}
const done = (name) => ev.steps.find((s) => s.step === name);
function record(entry) {
  ev.steps.push({ at: new Date().toISOString(), ...entry });
  save();
  say(`- **${entry.step}**: ${entry.summary}${entry.tx_hash ? ` ([tx](${EXPLORER}${entry.tx_hash}))` : ""}`);
  return entry;
}
function say(line) {
  console.log(line);
  fs.mkdirSync(outDir, { recursive: true });
  fs.appendFileSync(TRANSCRIPT, line + "\n");
}

// ----- keys (never printed, never in the evidence) ----------------------------
const keyFile = (role) => path.join(KEYS, role + ".json");
function newKey(role, accountId) {
  fs.mkdirSync(KEYS, { recursive: true });
  if (fs.existsSync(keyFile(role))) return JSON.parse(fs.readFileSync(keyFile(role), "utf8"));
  const pair = n.KeyPair.fromRandom("ed25519");
  const key = { role, account_id: accountId, public_key: pair.getPublicKey().toString(), secret_key: pair.toString() };
  fs.writeFileSync(keyFile(role), JSON.stringify(key, null, 2) + "\n", { mode: 0o600 });
  return key;
}
const loadKey = (role) => JSON.parse(fs.readFileSync(keyFile(role), "utf8"));
const accountFor = (role) => {
  const key = loadKey(role);
  return new n.Account(key.account_id, provider, key.secret_key);
};
const redact = (s) => String(s).replace(/ed25519:[1-9A-HJ-NP-Za-km-z]{30,}/g, "ed25519:<key>");

// ----- chain helpers ----------------------------------------------------------
async function rpc(method, params) {
  for (let attempt = 0; ; attempt++) {
    const response = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: "demo", method, params }) });
    const body = response.status === 429 ? null : await response.json();
    if (body && !(body.error && body.error.code === -429)) return body;
    if (attempt >= 5) throw new Error("the RPC endpoint kept rate-limiting " + method);
    await sleep(5000 * 2 ** attempt);
  }
}
async function view(contract, method, a) {
  const r = await rpc("query", { request_type: "call_function", finality: "final", account_id: contract, method_name: method, args_base64: Buffer.from(JSON.stringify(a)).toString("base64") });
  if (r.error || r.result.error) throw new Error("view " + method + " failed: " + JSON.stringify(r.error || r.result.error).slice(0, 200));
  return JSON.parse(Buffer.from(r.result.result).toString("utf8"));
}
async function accountExists(id) {
  const r = await rpc("query", { request_type: "view_account", finality: "final", account_id: id });
  return !!(r.result && !r.result.error);
}
async function accessKeys(id) {
  const r = await rpc("query", { request_type: "view_access_key_list", finality: "final", account_id: id });
  return r.result ? r.result.keys : [];
}
const balanceOf = async (id) => BigInt(await view(ev.accounts.token, "ft_balance_of", { account_id: id }));
const agentView = () => view(ev.accounts.wallet, "get_agent", { agent_id: ev.accounts.agent });

function summarize(result) {
  const all = JSON.stringify(result);
  const outcomes = result.receipts_outcome || [];
  const events = outcomes
    .flatMap((o) => o.outcome.logs)
    .filter((l) => l.startsWith("EVENT_JSON:"))
    .map((l) => {
      try {
        const e = JSON.parse(l.slice(11));
        return e.standard + ":" + e.event;
      } catch {
        return "unparsed";
      }
    });
  return {
    tx_hash: result.transaction.hash,
    outcome: result.status && result.status.Failure ? "failure" : "success",
    error_code: (all.match(/ERR_[A-Z_]+/) || [null])[0],
    events,
  };
}
async function send(role, receiverId, actions) {
  const result = await accountFor(role).signAndSendTransaction({ receiverId, actions, waitUntil: "FINAL", throwOnFailure: false });
  return summarize(result);
}
// A setup transaction that must succeed.
async function must(name, role, receiverId, actions, summary) {
  if (done(name)) return done(name);
  const s = await send(role, receiverId, actions);
  if (s.outcome !== "success") {
    record({ step: name, ...s, summary: "FAILED: " + summary });
    fail(name + " failed (" + (s.error_code || "see the explorer") + ")");
  }
  return record({ step: name, signer: loadKey(role).account_id, receiver: receiverId, ...s, summary });
}
// A transaction the node itself must refuse before execution.
async function nodeRejects(role, receiverId, actions) {
  try {
    const s = await send(role, receiverId, actions);
    return { rejected: false, ...s };
  } catch (e) {
    const raw = redact(String((e && (e.message || e.type)) || e));
    if (/429|rate limit|ECONN|fetch failed|timeout/i.test(raw)) throw e;
    return { rejected: true, error: raw.slice(0, 220) };
  }
}

async function assertTestnet() {
  const status = await rpc("status", []);
  if (!status.result || status.result.chain_id !== "testnet") fail("the RPC endpoint is not NEAR testnet");
}

// ----- setup --------------------------------------------------------------------
async function setup() {
  await assertTestnet();
  const funderFile = flag("funder-key");
  if (!funderFile) fail("--funder-key FILE is required: a funded testnet account's key file {account_id, secret_key}");
  const funder = JSON.parse(fs.readFileSync(path.resolve(funderFile), "utf8"));
  fs.mkdirSync(KEYS, { recursive: true });
  if (!fs.existsSync(keyFile("funder"))) fs.writeFileSync(keyFile("funder"), JSON.stringify({ role: "funder", account_id: funder.account_id, public_key: funder.public_key, secret_key: funder.secret_key }, null, 2) + "\n", { mode: 0o600 });

  const walletWasm = fs.readFileSync(path.join(REPO, "contracts", "target", "near", "policy_wallet", "policy_wallet.wasm"));
  const tokenWasm = fs.readFileSync(path.join(REPO, "contracts", "target", "near", "test_token", "test_token.wasm"));
  const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
  ev.wasm = { policy_wallet: { bytes: walletWasm.length, sha256: sha(walletWasm) }, test_token: { bytes: tokenWasm.length, sha256: sha(tokenWasm) } };

  if (!ev.tag) ev.tag = crypto.randomBytes(3).toString("hex");
  const stake = (bytes) => BigInt(bytes) * 10_000_000_000_000_000_000n;
  const plan = {
    owner: NEAR("0.5"),
    agent: NEAR("1"),
    gateway: NEAR("0.05"),
    token: stake(tokenWasm.length) + NEAR("0.15"),
    wallet: stake(walletWasm.length) + NEAR("0.1"),
  };
  ev.accounts.funder = funder.account_id;
  ev.accounts.mallory = `tpd-${ev.tag}-mallory.testnet`; // never created: the wallet refuses before any transfer
  save();
  if (!done("header")) {
    fs.writeFileSync(TRANSCRIPT, "");
    say("# TriagePay agent demo: transcript\n");
    say(`NEAR testnet, zero value, throwaway accounts. RPC ${RPC}. Started ${ev.started_at}.\n`);
    say("## Setup\n");
    ev.steps.push({ step: "header" });
    save();
  }
  for (const role of ["owner", "agent", "gateway", "token", "wallet"]) {
    const id = `tpd-${ev.tag}-${role}.testnet`;
    const key = newKey(role, id);
    ev.accounts[role] = id;
    save();
    if (!done("create_" + role) && (await accountExists(id))) {
      ev.steps.push({ step: "create_" + role, note: "already on chain" });
      save();
    }
    await must("create_" + role, "funder", REGISTRAR, [fc("create_account", { new_account_id: id, new_public_key: key.public_key }, 100, plan[role])], `create \`${id}\` with ${n.formatNearAmount(plan[role].toString(), 3)} NEAR`);
  }
  const A = ev.accounts;
  await must("deploy_token", "token", A.token, [n.actions.deployContract(new Uint8Array(tokenWasm)), fc("new", { owner_id: A.owner, total_supply: SUPPLY.toString() }, 30)], `deploy the test token (NEP-141, 6 decimals, emits NEP-297 events) and mint the supply to the owner`);
  const walletKey = loadKey("wallet");
  await must("deploy_wallet", "wallet", A.wallet, [n.actions.deployContract(new Uint8Array(walletWasm)), fc("new", { owner: A.owner, tokens: [A.token] }, 30), n.actions.deleteKey(n.PublicKey.fromString(walletKey.public_key))], "deploy the policy wallet, `new(owner, [token])`, and delete the wallet's own key, in one batch");
  await must("register_storage", "owner", A.token, [fc("storage_deposit", { account_id: A.wallet }, 30, NEAR("0.00125")), fc("storage_deposit", { account_id: A.gateway }, 30, NEAR("0.00125"))], "owner registers the wallet and the gateway on the token");
  await must("fund_wallet", "owner", A.token, [fc("ft_transfer_call", { receiver_id: A.wallet, amount: FUNDING.toString(), msg: "" }, 100, 1n)], `owner funds the wallet with ${units(FUNDING)} tokens`);
  const daily = PRICE * CALLS_PER_DAY;
  ev.policy = { per_call_max: PRICE.toString(), daily_max: daily.toString(), payees: [A.gateway] };
  await must("set_agent", "owner", A.wallet, [fc("set_agent", { agent_id: A.agent, per_call_max: PRICE.toString(), daily_max: daily.toString(), payees: [A.gateway] }, 30, 1n)], `owner sets the agent's policy: ${units(PRICE)} per call, ${units(daily)} per UTC day, payees \`[${A.gateway}]\``);
  const fcKey = newKey("agent_fc", A.agent);
  await must("add_agent_key", "agent", A.agent, [n.actions.addFunctionCallAccessKey(n.PublicKey.fromString(fcKey.public_key), A.wallet, ["pay"], NEAR(ALLOWANCE))], "the agent account adds a function-call key: receiver = the wallet, methods = `[pay]`");
  if (!done("keys_view")) {
    const walletKeys = await accessKeys(A.wallet);
    const agentKeys = await accessKeys(A.agent);
    const fcView = agentKeys.find((k) => k.public_key === fcKey.public_key);
    record({ step: "keys_view", wallet_access_keys: walletKeys.length, agent_function_call_key: fcView && fcView.access_key.permission, wallet_balance: (await balanceOf(A.wallet)).toString(), summary: `on chain: the wallet has ${walletKeys.length} access keys; the agent's key permission is \`${JSON.stringify(fcView && fcView.access_key.permission)}\`; the wallet holds ${units(await balanceOf(A.wallet))} tokens` });
  }
  say("\nStart the API with:\n");
  say("```");
  say(`APS_PROVIDER=near NEAR_NETWORK=testnet NEAR_GATEWAY_ACCOUNT=${A.gateway} NEAR_ASSET_CONTRACT=${A.token} cargo run -p aps-api --bin aps-api`);
  say("```\n");
}

// ----- the agent ----------------------------------------------------------------
const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64");
const unb64 = (raw) => JSON.parse(Buffer.from(raw, "base64").toString("utf8"));

async function requireScan(api, envelope) {
  const response = await fetch(api + "/v2/scan", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(envelope) });
  if (response.status !== 402) throw new Error("expected 402, got " + response.status);
  const required = unb64(response.headers.get("payment-required"));
  const requirement = required.accepts[0];
  if (requirement.network !== "near:testnet") throw new Error("refusing a requirement that is not on testnet");
  return requirement;
}
// The only thing the agent's key can do: call `pay` on the wallet.
async function walletPay(receiver, amount, memo) {
  return send("agent_fc", ev.accounts.wallet, [fc("pay", { token: ev.accounts.token, receiver_id: receiver, amount: amount.toString(), memo }, 50)]);
}
async function present(api, envelope, requirement, txHash) {
  const signature = b64({ x402Version: 2, resource: { url: "/v2/scan" }, accepted: requirement, payload: { network: requirement.network, intent_id: requirement.extra.memo, tx_hash: txHash, signer_id: ev.accounts.agent } });
  for (let attempt = 0; attempt < 40; attempt++) {
    const response = await fetch(api + "/v2/scan", { method: "POST", headers: { "content-type": "application/json", "PAYMENT-SIGNATURE": signature }, body: JSON.stringify(envelope) });
    const body = await response.json();
    if (response.status === 200) return { status: 200, body, payment: unb64(response.headers.get("payment-response")) };
    const reason = body && body.error && body.error.code;
    if (reason !== "settlement_pending") return { status: response.status, body, reason };
    await sleep(3000);
  }
  return { status: 402, reason: "settlement_pending" };
}
function loadFindings() {
  const dir = path.join(HERE, "findings");
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => ({ name: f.replace(/\.json$/, ""), envelope: JSON.parse(fs.readFileSync(path.join(dir, f), "utf8").replaceAll("{{MALLORY}}", ev.accounts.mallory)) }));
}

async function paidScan(api, name, envelope) {
  const step = "scan:" + name;
  if (done(step)) return done(step);
  const requirement = await requireScan(api, envelope);
  const before = await balanceOf(ev.accounts.wallet);
  const paid = await walletPay(requirement.payTo, BigInt(requirement.amount), requirement.extra.memo);
  if (paid.outcome !== "success" || !paid.events.includes("aps_policy_wallet:pay_settled")) {
    const after = await balanceOf(ev.accounts.wallet);
    return record({ step, requirement_memo: requirement.extra.memo, ...paid, wallet_balance_change: (after - before).toString(), summary: `402 received; the wallet REFUSED the payment with \`${paid.error_code}\`; wallet balance change ${after - before}; no verdict` });
  }
  const result = await present(api, envelope, requirement, paid.tx_hash);
  if (result.status !== 200) return record({ step, ...paid, summary: `paid, but the server answered ${result.status} ${result.reason}` });
  const report = result.body.report;
  const policy = await agentView();
  // triage.v2 only: the Stage B model judge's advisory call.
  const stageB = report.stage_b && report.stage_b.status === "ok" ? report.stage_b : null;
  const model = stageB
    ? `; model judge (${report.model_id}${report.model_verifiable ? ", TEE-listed" : ""}): ${stageB.severity_call}, p=${stageB.bountiable_probability}`
    : "";
  const entry = record({
    step,
    requirement_memo: requirement.extra.memo,
    ...paid,
    payment_response: result.payment,
    stage_b: report.stage_b || null,
    model_id: report.model_id || null,
    decision: report.decision,
    missing_evidence: report.missing_evidence,
    failed_gates: report.failed_gates,
    contains_embedded_instructions: report.contains_embedded_instructions,
    content_hash: report.content_hash,
    spent_today: policy.spent_in_window,
    remaining_today: policy.remaining,
    summary: `402, wallet \`pay\` of ${units(requirement.amount)}, proof accepted, verdict **${report.decision}**${report.missing_evidence.length ? ` (missing: ${report.missing_evidence.join(", ")})` : ""}${report.failed_gates.length ? ` (failed: ${report.failed_gates.join(", ")})` : ""}${report.contains_embedded_instructions ? "; the report flags embedded instructions" : ""}${model}; payer on chain \`${result.payment.payer}\`; remaining today ${units(policy.remaining)}`,
  });
  if (stageB && stageB.public_rationale) say(`  - model rationale: "${stageB.public_rationale}"`);
  return entry;
}

// --pause: wait for Enter before each step, so a presenter can talk over
// the run while it is recorded. Without it the run goes straight through.
const PAUSE = args.includes("--pause");
async function beat(title) {
  console.log("\n>>> " + title);
  if (!PAUSE) return;
  process.stdout.write("    (press Enter) ");
  await new Promise((resolve) => {
    process.stdin.resume();
    process.stdin.once("data", () => {
      process.stdin.pause();
      resolve();
    });
  });
}

async function run() {
  await assertTestnet();
  const api = (flag("api") || "").replace(/\/+$/, "");
  if (!api) fail("--api is required");
  const A = ev.accounts;
  if (!done("run_header")) {
    say("## The agent pays per call\n");
    ev.steps.push({ step: "run_header" });
    save();
  }
  const findings = loadFindings();
  const calls = [
    "Call 1: a finding whose deployment evidence is a placeholder",
    "Call 2: the same finding with a well-formed reference",
    "Call 3: a finding with an instruction hidden in it",
    "Call 4: a finding that needs the owner's key",
  ];
  for (const [i, finding] of findings.entries()) {
    if (!done("scan:" + finding.name)) await beat(calls[i] || finding.name);
    await paidScan(api, finding.name, finding.envelope);
  }

  if (!done("attacks_header")) {
    say("\n## What the key cannot do\n");
    ev.steps.push({ step: "attacks_header" });
    save();
  }
  // 1. The naive agent follows the instruction embedded in finding 3.
  if (!done("injected_payment")) await beat("The agent obeys the instruction hidden in finding 3");
  if (!done("injected_payment")) {
    const injected = findings.find((f) => f.envelope.reproduction_summary.includes(A.mallory));
    const before = await balanceOf(A.wallet);
    const s = await walletPay(A.mallory, PRICE, "unlock");
    const after = await balanceOf(A.wallet);
    record({ step: "injected_payment", instruction_source: injected && injected.name, attempted_receiver: A.mallory, ...s, wallet_balance_change: (after - before).toString(), summary: `the agent follows the instruction hidden in finding 3 and calls \`pay(receiver = ${A.mallory})\`: refused with \`${s.error_code}\`; wallet balance change ${after - before}` });
  }
  // 2. More than the per-call limit, to the allowlisted payee.
  if (!done("over_per_call")) await beat("One atomic unit over the per-call limit");
  if (!done("over_per_call")) {
    const before = await balanceOf(A.wallet);
    const s = await walletPay(A.gateway, PRICE + 1n, "too-much");
    const after = await balanceOf(A.wallet);
    record({ step: "over_per_call", ...s, wallet_balance_change: (after - before).toString(), summary: `\`pay\` of ${units(PRICE + 1n)} (one atomic unit over the per-call limit): refused with \`${s.error_code}\`; wallet balance change ${after - before}` });
  }
  // 3. A fifth call on the same UTC day.
  if (!done("scan:over-daily-limit")) await beat("A fifth call on the same UTC day");
  await paidScan(api, "over-daily-limit", findings[0].envelope);
  // 4. The agent tries to raise its own limit.
  if (!done("self_escalation")) await beat("The agent tries to raise its own limit");
  if (!done("self_escalation")) {
    const r = await nodeRejects("agent_fc", A.wallet, [fc("set_agent", { agent_id: A.agent, per_call_max: "1000000000", daily_max: "1000000000", payees: [A.mallory] }, 30, 1n)]);
    record({ step: "self_escalation", ...r, summary: r.rejected ? `the agent key calls the owner method \`set_agent\` with the required 1 yoctoNEAR: the node rejects the transaction before execution (\`${r.error}\`)` : `UNEXPECTED: the node accepted the transaction (${r.outcome}, ${r.error_code})` });
  }
  if (!done("self_escalation_no_deposit")) {
    const s = await nodeRejects("agent_fc", A.wallet, [fc("set_agent", { agent_id: A.agent, per_call_max: "1000000000", daily_max: "1000000000", payees: [A.mallory] }, 30, 0n)]);
    record({ step: "self_escalation_no_deposit", ...s, summary: s.rejected ? `the same call with no deposit: rejected by the node (\`${s.error}\`)` : `the same call with no deposit: the transaction ran and the contract refused it (${s.error_code || "the 1-yocto assertion"})` });
  }
  const policy = await agentView();
  say(`\nState after the run: spent today ${units(policy.spent_in_window)} of ${units(policy.daily_max)}; wallet holds ${units(await balanceOf(A.wallet))}; gateway holds ${units(await balanceOf(A.gateway))}.\n`);
}

// ----- a direct payment, and a replay ---------------------------------------------
async function direct() {
  await assertTestnet();
  const api = (flag("api") || "").replace(/\/+$/, "");
  if (!api) fail("--api is required");
  const A = ev.accounts;
  if (done("direct_payment")) return;
  await beat("A direct payment from a full-access account, then a replay");
  say("\n## A direct payment verifies the same way\n");
  const envelope = loadFindings()[1].envelope;
  envelope.title = envelope.title + " (direct payment)";
  const requirement = await requireScan(api, envelope);
  const paid = await send("owner", A.token, [fc("ft_transfer", { receiver_id: requirement.payTo, amount: requirement.amount, memo: requirement.extra.memo }, 30, 1n)]);
  if (paid.outcome !== "success") fail("the direct ft_transfer failed");
  const signature = (tx) => b64({ x402Version: 2, resource: { url: "/v2/scan" }, accepted: requirement, payload: { network: requirement.network, intent_id: requirement.extra.memo, tx_hash: tx, signer_id: A.owner } });
  const post = async () => {
    for (let attempt = 0; attempt < 40; attempt++) {
      const response = await fetch(api + "/v2/scan", { method: "POST", headers: { "content-type": "application/json", "PAYMENT-SIGNATURE": signature(paid.tx_hash) }, body: JSON.stringify(envelope) });
      const body = await response.json();
      if (!(response.status === 402 && body.error && body.error.code === "settlement_pending")) return { status: response.status, body, header: response.headers.get("payment-response") };
      await sleep(3000);
    }
    return { status: 402, body: {} };
  };
  const first = await post();
  const replay = await post();
  const identical = first.status === 200 && replay.status === 200 && JSON.stringify(first.body) === JSON.stringify(replay.body);
  const payment = first.header ? unb64(first.header) : {};
  record({ step: "direct_payment", requirement_memo: requirement.extra.memo, ...paid, payment_response: payment, decision: first.body.report && first.body.report.decision, replay_identical: identical, summary: `the owner pays with a plain \`ft_transfer\` (full-access key, 1 yoctoNEAR): proof accepted, verdict **${first.body.report && first.body.report.decision}**, payer on chain \`${payment.payer}\`; the same proof and request sent again returns ${identical ? "the identical response, with no second charge" : "status " + replay.status}` });
  const other = { ...envelope, title: "A different finding" };
  const reuse = await fetch(api + "/v2/scan", { method: "POST", headers: { "content-type": "application/json", "PAYMENT-SIGNATURE": signature(paid.tx_hash) }, body: JSON.stringify(other) });
  const reuseBody = await reuse.json();
  record({ step: "proof_reuse", status: reuse.status, reason: reuseBody.error && reuseBody.error.code, summary: `the same proof with a different request body: HTTP ${reuse.status} \`${reuseBody.error && reuseBody.error.code}\`` });
}

// ----- the owner cuts the agent off ---------------------------------------------
async function revoke() {
  await assertTestnet();
  const api = (flag("api") || "").replace(/\/+$/, "");
  const A = ev.accounts;
  if (!done("revoke_header")) {
    say("## The owner revokes the agent\n");
    ev.steps.push({ step: "revoke_header" });
    save();
  }
  if (!done("revoke_agent")) await beat("The owner revokes the agent");
  await must("revoke_agent", "owner", A.wallet, [fc("revoke_agent", { agent_id: A.agent }, 30, 1n)], "owner calls `revoke_agent` (1 yoctoNEAR)");
  if (!done("pay_after_revoke")) await beat("The revoked agent tries to pay; its key still exists");
  if (!done("pay_after_revoke")) {
    const before = await balanceOf(A.wallet);
    const s = await walletPay(A.gateway, 1n, "after-revoke");
    const after = await balanceOf(A.wallet);
    record({ step: "pay_after_revoke", ...s, wallet_balance_change: (after - before).toString(), summary: `the agent's key still exists and calls \`pay\` for one atomic unit: refused with \`${s.error_code}\`; wallet balance change ${after - before}` });
  }
  const fcKey = loadKey("agent_fc");
  if (!done("delete_agent_key")) await beat("The agent account deletes the key");
  await must("delete_agent_key", "agent", A.agent, [n.actions.deleteKey(n.PublicKey.fromString(fcKey.public_key))], "the agent account's full-access key deletes the function-call key (the wallet owner cannot do this; revocation at the wallet does not depend on it)");
  if (!done("pay_with_deleted_key")) {
    const r = await nodeRejects("agent_fc", A.wallet, [fc("pay", { token: A.token, receiver_id: A.gateway, amount: "1", memo: "deleted-key" }, 50)]);
    record({ step: "pay_with_deleted_key", ...r, summary: r.rejected ? `\`pay\` signed with the deleted key: rejected by the node (\`${r.error}\`)` : "UNEXPECTED: the node accepted a transaction signed with a deleted key" });
  }
  if (api && !done("ledger_after")) {
    const balances = await (await fetch(api + "/accounts/x402:dev-agent")).json();
    record({ step: "ledger_after", balances, summary: `server ledger for the x402 account: deposited ${balances.deposited_total && balances.deposited_total.decimal}, settled ${balances.settled_total && balances.settled_total.decimal}, available ${balances.available && balances.available.decimal}` });
  }
  say(`\nFinished ${new Date().toISOString()}.`);
}

try {
  if (command === "setup") await setup();
  else if (command === "run") await run();
  else if (command === "direct") await direct();
  else if (command === "revoke") await revoke();
  else fail("usage: demo.mjs setup|run|direct|revoke --out-dir DIR [--funder-key FILE] [--api URL]");
} catch (error) {
  console.error("ERROR: " + redact(String((error && error.stack) || error)).slice(0, 1500));
  process.exit(1);
}
