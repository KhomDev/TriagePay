// TriagePay on NEAR mainnet: a small, real proof run.
//
//   node scripts/mainnet/mainnet-proof.mjs setup  --out-dir DIR --keys-dir KDIR
//   node scripts/mainnet/mainnet-proof.mjs paid   --out-dir DIR --keys-dir KDIR [--api http://127.0.0.1:PORT]
//   node scripts/mainnet/mainnet-proof.mjs refuse --out-dir DIR --keys-dir KDIR
//   node scripts/mainnet/mainnet-proof.mjs revoke --out-dir DIR --keys-dir KDIR
//   node scripts/mainnet/mainnet-proof.mjs status --out-dir DIR --keys-dir KDIR
//   node scripts/mainnet/mainnet-proof.mjs close  --out-dir DIR --keys-dir KDIR   (later: recovers the wallet's deposit)
//
// The owner is an existing funded mainnet account. Its key is read from the
// file named by APS_MAINNET_ENV (lines NEAR_ACCOUNT_ID=... and
// NEAR_PRIVATE_KEY=...). The key is never printed, logged or written.
// Generated keys (the wallet's temporary deploy key, the agent's pay-only
// key) go to KDIR, which must be outside the repository. Evidence goes to
// DIR/mainnet-EVIDENCE.json and DIR/transcript.md, with keys redacted.
//
// Layout (all sub-accounts of the owner):
//   wallet  tp-wallet.<owner>  policy wallet; its deploy key is deleted in the deploy batch
//   agent   tp-agent2.<owner>  created with ONE key: a function-call key for `pay` on the wallet
//   payee   gateway.<owner>    existing account; the only allowlisted payee
//   token   wrap.near          wNEAR (NEP-141, 24 decimals)
//   mallory tp-mallory.<owner> never created; the wallet refuses before any transfer
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

const RPC = process.env.APS_MAINNET_RPC || "https://free.rpc.fastnear.com";
const EXPLORER = "https://nearblocks.io/txns/";
const TOKEN = "wrap.near";
const PRICE = 1_000_000n; // atomic units of wNEAR (1e-18 wNEAR): the API's fixed price of one call
const DAILY = PRICE * 4n;
const WRAP = "0.002"; // NEAR wrapped into wNEAR by the owner
const FUNDING = 1_000_000_000_000_000_000_000n; // 0.001 wNEAR into the wallet
const WALLET_NEAR = "1.765"; // covers 174,803 bytes of code (1.748 NEAR) plus account and state
const WALLET_TOP_UP = "0.045"; // second transfer after the first deploy ran short
const AGENT_NEAR = "0.07";
const AGENT_ALLOWANCE = "0.15"; // NEAR the pay-only key may commit to gas
// A function-call key's allowance is charged the full prepaid gas at the
// pessimistic price (about 10x the gas price on mainnet, ~0.001 NEAR per
// TGas) and refunds go to the account balance, not back to the allowance.
// The first agent, tp-agent, got 0.04 NEAR, less than one 50 TGas call
// (0.0509 NEAR); with no full-access key its key cannot be changed, so it
// was retired and tp-agent2 created with room for several 40 TGas calls.
const PAY_TGAS = 40;
const RETIRED_AGENT_PREFIX = "tp-agent";

const args = process.argv.slice(2);
const command = args[0];
const flag = (name) => {
  const i = args.indexOf("--" + name);
  return i >= 0 ? args[i + 1] : undefined;
};
function fail(message) {
  console.error("STOP: " + message);
  process.exit(2);
}
const outDir = flag("out-dir") && path.resolve(flag("out-dir"));
const keysDir = flag("keys-dir") && path.resolve(flag("keys-dir"));
if (!outDir || !keysDir) fail("--out-dir and --keys-dir are required");
const relKeys = path.relative(REPO, keysDir);
if (relKeys === "" || (!relKeys.startsWith("..") && !path.isAbsolute(relKeys))) fail("--keys-dir must be outside the repository");
const EVIDENCE = path.join(outDir, "mainnet-EVIDENCE.json");
const TRANSCRIPT = path.join(outDir, "transcript.md");

const T = (tgas) => BigInt(tgas) * 1_000_000_000_000n;
const NEAR = (s) => BigInt(n.parseNearAmount(s));
const fmtNear = (y) => n.formatNearAmount(BigInt(y).toString(), 6);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const provider = new n.JsonRpcProvider({ url: RPC });
const fc = (method, a, tgas, deposit) => n.actions.functionCall(method, a, T(tgas), deposit || 0n);
const redact = (s) => String(s).replace(/(ed25519:)?[1-9A-HJ-NP-Za-km-z]{80,}/g, "<redacted>");

// ----- owner key: read from a file, never printed ------------------------------
function loadOwner() {
  const file = process.env.APS_MAINNET_ENV;
  if (!file) fail("set APS_MAINNET_ENV to the file holding NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY");
  const vars = {};
  for (const line of fs.readFileSync(file, "utf8").split(/\r?\n/)) {
    const m = line.match(/^\s*(NEAR_ACCOUNT_ID|NEAR_PRIVATE_KEY)\s*=\s*"?([^"\s]+)"?\s*$/);
    if (m) vars[m[1]] = m[2];
  }
  if (!vars.NEAR_ACCOUNT_ID || !vars.NEAR_PRIVATE_KEY) fail("the env file lacks NEAR_ACCOUNT_ID or NEAR_PRIVATE_KEY");
  const secret = vars.NEAR_PRIVATE_KEY.startsWith("ed25519:") ? vars.NEAR_PRIVATE_KEY : "ed25519:" + vars.NEAR_PRIVATE_KEY;
  const pair = n.KeyPair.fromString(secret);
  return { account_id: vars.NEAR_ACCOUNT_ID, public_key: pair.getPublicKey().toString(), secret };
}
const OWNER = loadOwner();

// ----- generated keys (KDIR only) -------------------------------------------------
const keyFile = (role) => path.join(keysDir, role + ".json");
function newKey(role, accountId) {
  fs.mkdirSync(keysDir, { recursive: true });
  if (fs.existsSync(keyFile(role))) return JSON.parse(fs.readFileSync(keyFile(role), "utf8"));
  const pair = n.KeyPair.fromRandom("ed25519");
  const key = { role, account_id: accountId, public_key: pair.getPublicKey().toString(), secret_key: pair.toString() };
  fs.writeFileSync(keyFile(role), JSON.stringify(key, null, 2) + "\n", { mode: 0o600 });
  return key;
}
const loadKey = (role) => JSON.parse(fs.readFileSync(keyFile(role), "utf8"));
function signer(role) {
  if (role === "owner") return new n.Account(OWNER.account_id, provider, OWNER.secret);
  const key = loadKey(role);
  return new n.Account(key.account_id, provider, key.secret_key);
}

// ----- evidence ---------------------------------------------------------------------
const A = {
  owner: OWNER.account_id,
  wallet: `tp-wallet.${OWNER.account_id}`,
  agent: `tp-agent2.${OWNER.account_id}`,
  retired_agent: `${RETIRED_AGENT_PREFIX}.${OWNER.account_id}`,
  payee: `gateway.${OWNER.account_id}`,
  token: TOKEN,
  mallory: `tp-mallory.${OWNER.account_id}`,
};
let ev = fs.existsSync(EVIDENCE)
  ? JSON.parse(fs.readFileSync(EVIDENCE, "utf8"))
  : { run: "TriagePay mainnet proof", network: "near:mainnet", rpc: RPC, explorer: EXPLORER, value: "real (wNEAR, tiny amounts)", started_at: new Date().toISOString(), tooling: "near-api-js 7.3.1, Node " + process.version, accounts: A, owner_public_key: OWNER.public_key, policy: {}, steps: [] };
ev.accounts = A;
function save() {
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(EVIDENCE, redact(JSON.stringify(ev, null, 2)) + "\n");
}
function say(line) {
  console.log(line);
  fs.mkdirSync(outDir, { recursive: true });
  fs.appendFileSync(TRANSCRIPT, redact(line) + "\n");
}
const done = (name) => ev.steps.find((s) => s.step === name);
function record(entry) {
  ev.steps.push({ at: new Date().toISOString(), ...entry });
  save();
  say(`- **${entry.step}**: ${entry.summary}${entry.tx_hash ? ` ([tx](${EXPLORER}${entry.tx_hash}))` : ""}`);
  return entry;
}

// ----- chain helpers ----------------------------------------------------------------
async function rpc(method, params) {
  for (let attempt = 0; ; attempt++) {
    const response = await fetch(RPC, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: "mainnet", method, params }) });
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
async function account(id) {
  const r = await rpc("query", { request_type: "view_account", finality: "final", account_id: id });
  return r.result && !r.result.error ? r.result : null;
}
async function accessKeys(id) {
  const r = await rpc("query", { request_type: "view_access_key_list", finality: "final", account_id: id });
  return r.result ? r.result.keys : [];
}
const balanceOf = async (id) => BigInt(await view(TOKEN, "ft_balance_of", { account_id: id }));
async function assertMainnet() {
  const status = await rpc("status", []);
  if (!status.result || status.result.chain_id !== "mainnet") fail("the RPC endpoint is not NEAR mainnet");
  const keys = await accessKeys(OWNER.account_id);
  const k = keys.find((x) => x.public_key === OWNER.public_key);
  if (!k || k.access_key.permission !== "FullAccess") fail("the owner key is not a full-access key of " + OWNER.account_id);
}
function summarize(result) {
  const all = JSON.stringify(result);
  const outcomes = result.receipts_outcome || [];
  const logs = outcomes.flatMap((o) => o.outcome.logs);
  const events = logs.filter((l) => l.startsWith("EVENT_JSON:")).map((l) => {
    try {
      const e = JSON.parse(l.slice(11));
      return e.standard + ":" + e.event;
    } catch {
      return "unparsed";
    }
  });
  const burnt = BigInt(result.transaction_outcome.outcome.tokens_burnt) + outcomes.reduce((s, o) => s + BigInt(o.outcome.tokens_burnt), 0n);
  return {
    tx_hash: result.transaction.hash,
    outcome: result.status && result.status.Failure ? "failure" : "success",
    error_code: (all.match(/ERR_[A-Z_]+/) || [null])[0],
    events,
    gas_burnt_near: fmtNear(burnt),
  };
}
async function send(role, receiverId, actions) {
  const result = await signer(role).signAndSendTransaction({ receiverId, actions, waitUntil: "FINAL", throwOnFailure: false });
  return summarize(result);
}
async function must(name, role, receiverId, actions, summary) {
  if (done(name)) return done(name);
  const s = await send(role, receiverId, actions);
  if (s.outcome !== "success") {
    record({ step: name + "_failed", ...s, summary: "FAILED: " + summary });
    fail(name + " failed (" + (s.error_code || "see the explorer") + ")");
  }
  return record({ step: name, signer: role === "owner" ? OWNER.account_id : loadKey(role).account_id, receiver: receiverId, ...s, summary });
}
async function ownerBalance() {
  const a = await account(OWNER.account_id);
  return a ? a.amount : "0";
}

// ----- setup -------------------------------------------------------------------------
async function setup() {
  await assertMainnet();
  const wasm = fs.readFileSync(path.join(REPO, "contracts", "target", "near", "policy_wallet", "policy_wallet.wasm"));
  ev.wasm = { policy_wallet: { bytes: wasm.length, sha256: crypto.createHash("sha256").update(wasm).digest("hex") } };
  if (!ev.owner_balance_before) ev.owner_balance_before = await ownerBalance();
  save();
  if (!done("header")) {
    fs.writeFileSync(TRANSCRIPT, "");
    say("# TriagePay on NEAR mainnet: transcript\n");
    say(`NEAR mainnet, real value (tiny wNEAR amounts). RPC ${RPC}. Started ${ev.started_at}. Owner \`${A.owner}\` starts with ${fmtNear(ev.owner_balance_before)} NEAR.\n`);
    say("## Setup\n");
    ev.steps.push({ step: "header" });
    save();
  }
  const need = NEAR(WALLET_NEAR) + NEAR(AGENT_NEAR) + NEAR(WRAP) + NEAR("0.06");
  if (!done("create_wallet") && BigInt(await ownerBalance()) < need) fail(`owner balance below ${fmtNear(need)} NEAR`);
  if (!(await account(A.payee))) fail(`payee ${A.payee} does not exist`);

  const walletKey = newKey("wallet_deploy", A.wallet);
  await must("create_wallet", "owner", A.wallet, [n.actions.createAccount(), n.actions.transfer(NEAR(WALLET_NEAR)), n.actions.addFullAccessKey(n.PublicKey.fromString(walletKey.public_key))], `create \`${A.wallet}\` with ${WALLET_NEAR} NEAR and a temporary deploy key`);
  // The first deploy attempt on 2026-11-12 failed with LackBalanceForState (short by 0.0276 NEAR: code plus initial state is larger than the code alone); the batch reverted and this top-up followed.
  await must("top_up_wallet", "owner", A.wallet, [n.actions.transfer(NEAR(WALLET_TOP_UP))], `send ${WALLET_TOP_UP} NEAR more to \`${A.wallet}\` for code plus state storage`);
  await must("deploy_wallet", "wallet_deploy", A.wallet, [n.actions.deployContract(new Uint8Array(wasm)), fc("new", { owner: A.owner, tokens: [TOKEN] }, 30), n.actions.deleteKey(n.PublicKey.fromString(walletKey.public_key))], `deploy the policy wallet (${wasm.length} bytes), \`new(owner = ${A.owner}, tokens = [${TOKEN}])\`, and delete the wallet's only key, in one batch`);

  const agentKey = newKey("agent2_pay", A.agent);
  await must("create_agent", "owner", A.agent, [n.actions.createAccount(), n.actions.transfer(NEAR(AGENT_NEAR)), n.actions.addFunctionCallAccessKey(n.PublicKey.fromString(agentKey.public_key), A.wallet, ["pay"], NEAR(AGENT_ALLOWANCE))], `create \`${A.agent}\` with ${AGENT_NEAR} NEAR and exactly one key: a function-call key, receiver = the wallet, methods = \`[pay]\`, allowance ${AGENT_ALLOWANCE} NEAR (no full-access key)`);

  const reg = async (id) => (await view(TOKEN, "storage_balance_of", { account_id: id })) !== null;
  const wrapActions = [];
  if (!(await reg(A.owner))) wrapActions.push(fc("storage_deposit", { account_id: A.owner }, 10, NEAR("0.00125")));
  wrapActions.push(fc("near_deposit", {}, 10, NEAR(WRAP)));
  if (!(await reg(A.wallet))) wrapActions.push(fc("storage_deposit", { account_id: A.wallet }, 10, NEAR("0.00125")));
  if (!(await reg(A.payee))) wrapActions.push(fc("storage_deposit", { account_id: A.payee }, 10, NEAR("0.00125")));
  await must("wrap_and_register", "owner", TOKEN, wrapActions, `owner wraps ${WRAP} NEAR into wNEAR and registers the wallet and the payee on \`${TOKEN}\``);
  await must("fund_wallet", "owner", TOKEN, [fc("ft_transfer_call", { receiver_id: A.wallet, amount: FUNDING.toString(), msg: "" }, 100, 1n)], `owner funds the wallet with 0.001 wNEAR (\`ft_transfer_call\`, accepted by \`ft_on_transfer\`)`);
  ev.policy = { per_call_max: PRICE.toString(), daily_max: DAILY.toString(), payees: [A.payee], token: TOKEN, units: "yocto-wNEAR" };
  await must("set_agent", "owner", A.wallet, [fc("set_agent", { agent_id: A.agent, per_call_max: PRICE.toString(), daily_max: DAILY.toString(), payees: [A.payee] }, 30, 1n)], `owner sets the agent's policy: ${PRICE} yocto-wNEAR per call, ${DAILY} per UTC day, payees \`[${A.payee}]\``);
  if (!done("keys_view")) {
    const walletKeys = await accessKeys(A.wallet);
    const agentKeys = await accessKeys(A.agent);
    record({ step: "keys_view", wallet_access_keys: walletKeys.length, agent_access_keys: agentKeys.map((k) => ({ public_key: k.public_key, permission: k.access_key.permission })), wallet_wnear: (await balanceOf(A.wallet)).toString(), config: await view(A.wallet, "get_config", {}), summary: `on chain: the wallet has ${walletKeys.length} access keys; the agent has ${agentKeys.length} key(s), permission \`${JSON.stringify(agentKeys.map((k) => k.access_key.permission))}\`; the wallet holds ${await balanceOf(A.wallet)} yocto-wNEAR` });
  }
}

// ----- the agent pays -------------------------------------------------------------
const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64");
const unb64 = (raw) => JSON.parse(Buffer.from(raw, "base64").toString("utf8"));
const walletPay = (receiver, amount, memo) => send("agent2_pay", A.wallet, [fc("pay", { token: TOKEN, receiver_id: receiver, amount: amount.toString(), memo }, PAY_TGAS)]);

async function paid() {
  await assertMainnet();
  if (done("paid_call")) return;
  if (!done("pay_header")) {
    say("\n## The agent pays\n");
    ev.steps.push({ step: "pay_header" });
    save();
  }
  const api = (flag("api") || "").replace(/\/+$/, "");
  let requirement = null;
  let envelope = null;
  if (api) {
    const dir = path.join(REPO, "agents", "demo", "findings");
    const file = fs.readdirSync(dir).filter((f) => f.endsWith(".json")).sort()[1];
    envelope = JSON.parse(fs.readFileSync(path.join(dir, file), "utf8"));
    const response = await fetch(api + "/v2/scan", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(envelope) });
    if (response.status !== 402) fail("expected 402 from the API, got " + response.status);
    requirement = unb64(response.headers.get("payment-required")).accepts[0];
    if (requirement.network !== "near:mainnet" || requirement.asset !== TOKEN || requirement.payTo !== A.payee) fail("the 402 requirement is not near:mainnet / wrap.near / the payee: " + JSON.stringify(requirement).slice(0, 300));
  }
  const amount = requirement ? BigInt(requirement.amount) : PRICE;
  const memo = requirement ? requirement.extra.memo : "triagepay-mainnet-proof";
  const before = { wallet: await balanceOf(A.wallet), payee: await balanceOf(A.payee) };
  const s = await walletPay(A.payee, amount, memo);
  const after = { wallet: await balanceOf(A.wallet), payee: await balanceOf(A.payee) };
  const entry = { step: "paid_call", memo, amount: amount.toString(), ...s, wallet_balance_change: (after.wallet - before.wallet).toString(), payee_balance_change: (after.payee - before.payee).toString() };
  let verdict = "";
  if (requirement && s.outcome === "success") {
    const signature = b64({ x402Version: 2, resource: { url: "/v2/scan" }, accepted: requirement, payload: { network: requirement.network, intent_id: memo, tx_hash: s.tx_hash, signer_id: A.agent } });
    for (let attempt = 0; attempt < 40; attempt++) {
      const response = await fetch(api + "/v2/scan", { method: "POST", headers: { "content-type": "application/json", "PAYMENT-SIGNATURE": signature }, body: JSON.stringify(envelope) });
      const body = await response.json();
      const reason = body && body.error && body.error.code;
      if (response.status === 402 && reason === "settlement_pending") {
        await sleep(3000);
        continue;
      }
      entry.x402 = { status: response.status, reason: reason || null, decision: body.report && body.report.decision, payment_response: response.headers.get("payment-response") ? unb64(response.headers.get("payment-response")) : null, detail: reason ? JSON.stringify(body.error).slice(0, 300) : undefined };
      verdict = response.status === 200 ? `; x402 proof accepted by the API, verdict **${body.report.decision}**` : `; the API answered ${response.status} \`${reason}\``;
      break;
    }
  }
  record({ ...entry, summary: `${requirement ? "402 received; " : ""}the agent's pay-only key calls \`pay(${TOKEN}, ${A.payee}, ${amount})\`: ${s.outcome}${s.error_code ? " `" + s.error_code + "`" : ""}, events ${JSON.stringify(s.events)}; wallet ${after.wallet - before.wallet}, payee +${after.payee - before.payee} yocto-wNEAR${verdict}` });
}

async function refuse() {
  await assertMainnet();
  if (done("refused_payee")) return;
  if (!done("refuse_header")) {
    say("\n## What the key cannot do\n");
    ev.steps.push({ step: "refuse_header" });
    save();
  }
  const before = await balanceOf(A.wallet);
  const s = await walletPay(A.mallory, PRICE, "unlock");
  const after = await balanceOf(A.wallet);
  record({ step: "refused_payee", attempted_receiver: A.mallory, ...s, wallet_balance_change: (after - before).toString(), summary: `a prompt-injected instruction makes the agent call \`pay(receiver = ${A.mallory})\`: ${s.outcome === "failure" ? "refused" : "UNEXPECTED " + s.outcome} with \`${s.error_code}\`; wallet balance change ${after - before}` });
}

async function revoke() {
  await assertMainnet();
  if (!done("revoke_header")) {
    say("\n## The owner revokes the agent\n");
    ev.steps.push({ step: "revoke_header" });
    save();
  }
  await must("revoke_retired_agent", "owner", A.wallet, [fc("revoke_agent", { agent_id: A.retired_agent }, 30, 1n)], `owner revokes the retired first agent \`${A.retired_agent}\` (its key allowance was too small for one call)`);
  await must("revoke_agent", "owner", A.wallet, [fc("revoke_agent", { agent_id: A.agent }, 30, 1n)], `owner calls \`revoke_agent(${A.agent})\` (1 yoctoNEAR)`);
  if (!done("pay_after_revoke")) {
    const before = await balanceOf(A.wallet);
    const s = await walletPay(A.payee, 1n, "after-revoke");
    const after = await balanceOf(A.wallet);
    record({ step: "pay_after_revoke", ...s, wallet_balance_change: (after - before).toString(), summary: `the revoked agent's key still exists and calls \`pay\` to the allowlisted payee for 1 yocto-wNEAR: ${s.outcome === "failure" ? "refused" : "UNEXPECTED " + s.outcome} with \`${s.error_code}\`; wallet balance change ${after - before}` });
  }
}

async function status() {
  const ownerNow = await ownerBalance();
  const w = await account(A.wallet);
  const ag = await account(A.agent);
  const st = {
    owner_near: fmtNear(ownerNow),
    owner_spent_near: ev.owner_balance_before ? fmtNear(BigInt(ev.owner_balance_before) - BigInt(ownerNow)) : null,
    wallet_near: w ? fmtNear(w.amount) : null,
    wallet_storage_bytes: w ? w.storage_usage : null,
    agent_near: ag ? fmtNear(ag.amount) : null,
    wallet_wnear: (await balanceOf(A.wallet)).toString(),
    payee_wnear: (await balanceOf(A.payee)).toString(),
    agent: await view(A.wallet, "get_agent", { agent_id: A.agent }),
    wallet_keys: (await accessKeys(A.wallet)).length,
  };
  ev.final_state = { at: new Date().toISOString(), ...st };
  save();
  say(`\nState: owner ${st.owner_near} NEAR (down ${st.owner_spent_near}, of which ${WALLET_NEAR} + ${WALLET_TOP_UP} sits in the wallet account and ${AGENT_NEAR} in the agent account); wallet ${st.wallet_near} NEAR, ${st.wallet_wnear} yocto-wNEAR, ${st.wallet_keys} access keys; payee ${st.payee_wnear} yocto-wNEAR; agent active: ${st.agent && st.agent.active}.\n`);
}

// ----- later: close the wallet and recover its storage deposit ------------------
// Not run on 2026-11-12: the wallet stays live for judges. Order: pause,
// withdraw the whole wNEAR balance to the owner, then close; `close` deletes
// the account only when every allowlisted token balance is zero, and sends
// the account's NEAR (about 1.807) to the beneficiary.
async function close() {
  await assertMainnet();
  if (!done("close_header")) {
    say("\n## The owner closes the wallet\n");
    ev.steps.push({ step: "close_header" });
    save();
  }
  const config = await view(A.wallet, "get_config", {});
  if (!config.paused) await must("pause", "owner", A.wallet, [fc("pause", {}, 30, 1n)], "owner pauses the wallet");
  const held = await balanceOf(A.wallet);
  if (held > 0n) await must("withdraw_all", "owner", A.wallet, [fc("withdraw", { token: TOKEN, amount: held.toString(), to: A.owner }, 60, 1n)], `owner withdraws ${held} yocto-wNEAR to \`${A.owner}\``);
  await must("close", "owner", A.wallet, [fc("close", { beneficiary: A.owner }, 60, 1n)], `owner calls \`close(beneficiary = ${A.owner})\`: the wallet account is deleted and its NEAR returns to the owner`);
}
try {
  if (command === "setup") await setup();
  else if (command === "paid") await paid();
  else if (command === "refuse") await refuse();
  else if (command === "revoke") await revoke();
  else if (command === "status") await status();
  else if (command === "close") await close();
  else fail("usage: mainnet-proof.mjs setup|paid|refuse|revoke|status|close --out-dir DIR --keys-dir KDIR [--api URL]");
} catch (error) {
  console.error("ERROR: " + redact(String((error && error.stack) || error)).slice(0, 1500));
  process.exit(1);
}