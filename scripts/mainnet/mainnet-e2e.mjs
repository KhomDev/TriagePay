// Mainnet factory run. Budget is one factory deploy plus one wallet.
//
//   node scripts/mainnet/mainnet-e2e.mjs --dry-run
//   node scripts/mainnet/mainnet-e2e.mjs --out-dir DIR --keys-dir KDIR
//
// Dry-run prints the projected budget and does not read a key. A real run
// reads NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY from the file named by
// APS_MAINNET_ENV. The key is never printed, logged, or written.
// Generated keys go to KDIR, which must be outside the repository.
//
// The owner deploys triagepay.<owner> only when the owner is hlpok1lad.tg;
// otherwise the factory account is triagepay.<owner>. The fee recipient is
// the owner, so the 1 NEAR fee comes back. The script then creates one
// wallet, moves a small amount of USDC and wNEAR, runs the hold and revoke
// checks, and closes the wallet so its storage returns. A second wallet is
// not created: another copy costs the full create deposit, and the budget
// does not have it.
//
// Requires near-api-js 7.3.1: `npm ci --prefix clients/typescript`.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  BUDGET_CEILING_YOCTO,
  budgetLines,
  BYTE_YOCTO,
  createCostYocto,
  FACTORY_STATE_BYTES,
  FEE_YOCTO,
  projectBudget,
  WALLET_CODE_BYTES,
  WALLET_CODE_SHA256,
} from "./budget.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..", "..");
const USDC = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
const WRAP = "wrap.near";
const RPC = process.env.APS_MAINNET_RPC || "https://free.rpc.fastnear.com";
const EXPLORER = "https://nearblocks.io/txns/";
const USDC_AMOUNT = 10_000n; // 0.01 USDC
const WNEAR_AMOUNT = 1_000_000_000_000_000_000n; // 1e-6 wNEAR
const STORAGE_DEPOSIT = 1_250_000_000_000_000_000_000n; // 0.00125 NEAR
const AGENT_NEAR = 350_000_000_000_000_000_000_000n; // 0.35 NEAR
const AGENT_ALLOWANCE = 300_000_000_000_000_000_000_000n; // 0.30 NEAR, not refunded to the key
const ONE = 1n;

const args = process.argv.slice(2);
const dry = args.includes("--dry-run");
const flag = (name) => {
  const i = args.indexOf("--" + name);
  return i >= 0 ? args[i + 1] : undefined;
};
function fail(message) {
  console.error("STOP: " + message);
  process.exit(2);
}

function factoryWasmPath() {
  if (process.env.FACTORY_WASM) return process.env.FACTORY_WASM;
  const opt = path.join(REPO, "contracts", "target", "near", "wallet_factory", "wallet_factory.wasm");
  const raw = path.join(REPO, "contracts", "target", "wasm32-unknown-unknown", "release", "wallet_factory.wasm");
  if (fs.existsSync(opt)) return opt;
  if (fs.existsSync(raw)) return raw;
  return opt;
}

function printBudget(factoryBytes) {
  const budget = projectBudget(factoryBytes);
  console.log("TriagePay mainnet budget (dry-run)\n");
  for (const line of budgetLines(budget)) console.log(line);
  console.log("");
  console.log("Deploy, from contracts/, with the key only in the environment:");
  console.log("  rustup target add wasm32-unknown-unknown");
  console.log("  sudo apt-get install -y binaryen   # wasm-opt 108");
  console.log('  RUSTFLAGS="--cfg near --remap-path-prefix=${CARGO_HOME}=/cargo --remap-path-prefix=${RUSTUP_HOME}=/rustup --remap-path-prefix=/usr/local/cargo=/cargo --remap-path-prefix=/usr/local/rustup=/rustup --remap-path-prefix=$PWD=/src" \\');
  console.log("    cargo build -p policy-wallet -p wallet-factory --target wasm32-unknown-unknown --release");
  console.log("  wasm-opt -Oz --strip-debug --strip-producers --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \\");
  console.log("    target/wasm32-unknown-unknown/release/wallet_factory.wasm -o target/near/wallet_factory/wallet_factory.wasm");
  console.log("  APS_MAINNET_ENV=/path/outside/the/repo.env node scripts/mainnet/mainnet-e2e.mjs --out-dir DIR --keys-dir /path/outside/keys");
  console.log("");
  console.log(`Embedded wallet ${WALLET_CODE_BYTES} bytes, sha256 ${WALLET_CODE_SHA256}.`);
  console.log("The factory stores that wasm compressed and deploys the inflated bytes on each wallet.");
  console.log("close(beneficiary), after pause and a full withdraw, returns the wallet stake. The factory lock stays until the factory account is deleted.");
  if (!budget.fits) fail(`projected peak is over ${Number(BUDGET_CEILING_YOCTO) / 1e24} NEAR`);
  return budget;
}

const wasmFile = factoryWasmPath();
if (!fs.existsSync(wasmFile)) {
  fail(`missing factory wasm at ${wasmFile}. Build it with the commands this script prints, then re-run.`);
}
const factoryWasm = fs.readFileSync(wasmFile);
const budget = printBudget(factoryWasm.length);
if (dry) process.exit(0);

const requireFromClient = createRequire(path.join(REPO, "clients", "typescript", "package.json"));
const n = await import(pathToFileURL(requireFromClient.resolve("near-api-js")).href);

const outDir = flag("out-dir") && path.resolve(flag("out-dir"));
const keysDir = flag("keys-dir") && path.resolve(flag("keys-dir"));
if (!outDir || !keysDir) fail("--out-dir and --keys-dir are required");
const relKeys = path.relative(REPO, keysDir);
if (relKeys === "" || (!relKeys.startsWith("..") && !path.isAbsolute(relKeys))) fail("--keys-dir must be outside the repository");

const T = (tgas) => BigInt(tgas) * 1_000_000_000_000n;
const fmtNear = (y) => n.formatNearAmount(BigInt(y).toString(), 5);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const provider = new n.JsonRpcProvider({ url: RPC });
const fc = (method, a, tgas, deposit) => n.actions.functionCall(method, a, T(tgas), deposit || 0n);
const redact = (s) => String(s).replace(/(ed25519:)?[1-9A-HJ-NP-Za-km-z]{80,}/g, "<redacted>");

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
const FACTORY = OWNER.account_id === "hlpok1lad.tg" ? "triagepay.hlpok1lad.tg" : `triagepay.${OWNER.account_id}`;

const EVIDENCE = path.join(outDir, "mainnet-EVIDENCE.json");
const TRANSCRIPT = path.join(outDir, "transcript.md");
let ev = fs.existsSync(EVIDENCE)
  ? JSON.parse(fs.readFileSync(EVIDENCE, "utf8"))
  : {
      run: "TriagePay factory mainnet",
      network: "near:mainnet",
      rpc: RPC,
      started_at: new Date().toISOString(),
      owner: OWNER.account_id,
      factory: FACTORY,
      budget: { peak_yocto: budget.peak.toString(), ceiling_yocto: BUDGET_CEILING_YOCTO.toString() },
      steps: [],
    };
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

async function rpc(method, params) {
  for (let attempt = 0; ; attempt++) {
    const response = await fetch(RPC, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "mainnet", method, params }),
    });
    const body = response.status === 429 ? null : await response.json();
    if (body && !(body.error && body.error.code === -429)) return body;
    if (attempt >= 5) throw new Error("the RPC endpoint kept rate-limiting " + method);
    await sleep(5000 * 2 ** attempt);
  }
}
async function view(contract, method, a) {
  const r = await rpc("query", {
    request_type: "call_function",
    finality: "final",
    account_id: contract,
    method_name: method,
    args_base64: Buffer.from(JSON.stringify(a)).toString("base64"),
  });
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
const yoctoBalance = async (id) => BigInt((await account(id))?.amount || "0");
const ftBalance = async (token, id) => BigInt(await view(token, "ft_balance_of", { account_id: id }));

function summarize(result) {
  const all = JSON.stringify(result);
  const outcomes = result.receipts_outcome || [];
  const logs = outcomes.flatMap((o) => o.outcome.logs);
  const events = logs
    .filter((l) => l.startsWith("EVENT_JSON:"))
    .map((l) => {
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
function signer(role) {
  if (role === "owner") return new n.Account(OWNER.account_id, provider, OWNER.secret);
  const key = JSON.parse(fs.readFileSync(path.join(keysDir, role + ".json"), "utf8"));
  return new n.Account(key.account_id, provider, key.secret_key);
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
  return record({ step: name, ...s, summary });
}
function newKey(role, accountId) {
  fs.mkdirSync(keysDir, { recursive: true });
  const file = path.join(keysDir, role + ".json");
  if (fs.existsSync(file)) return JSON.parse(fs.readFileSync(file, "utf8"));
  const pair = n.KeyPair.fromRandom("ed25519");
  const key = { role, account_id: accountId, public_key: pair.getPublicKey().toString(), secret_key: pair.toString() };
  fs.writeFileSync(file, JSON.stringify(key, null, 2) + "\n", { mode: 0o600 });
  return key;
}
const loadKey = (role) => JSON.parse(fs.readFileSync(path.join(keysDir, role + ".json"), "utf8"));

async function assertMainnet() {
  const status = await rpc("status", []);
  if (!status.result || status.result.chain_id !== "mainnet") fail("the RPC endpoint is not NEAR mainnet");
  const keys = await accessKeys(OWNER.account_id);
  const k = keys.find((x) => x.public_key === OWNER.public_key);
  if (!k || k.access_key.permission !== "FullAccess") fail("the owner key is not a full-access key of " + OWNER.account_id);
}

const walletName = () => {
  if (!ev.wallet_name) ev.wallet_name = "e2e" + crypto.randomBytes(3).toString("hex");
  save();
  return ev.wallet_name;
};

async function main() {
  await assertMainnet();
  const start = await yoctoBalance(OWNER.account_id);
  if (!ev.owner_balance_before) ev.owner_balance_before = start.toString();
  save();
  if (!done("header")) {
    fs.writeFileSync(TRANSCRIPT, "");
    say("# TriagePay factory on NEAR mainnet\n");
    say(`Owner \`${OWNER.account_id}\` starts with ${fmtNear(start)} NEAR. Peak the script plans to draw before the fee returns: ${fmtNear(budget.peak)} NEAR. Ceiling ${fmtNear(BUDGET_CEILING_YOCTO)} NEAR.\n`);
    ev.steps.push({ step: "header" });
    save();
  }
  if (start < budget.peak && !done("deploy_factory")) fail(`owner has ${fmtNear(start)} NEAR, peak is ${fmtNear(budget.peak)} NEAR`);

  const existing = await account(FACTORY);
  if (!existing) {
    const fund = (BigInt(factoryWasm.length) + FACTORY_STATE_BYTES) * BYTE_YOCTO;
    const deployKey = newKey("factory_deploy", FACTORY);
    await must(
      "create_factory",
      "owner",
      FACTORY,
      [n.actions.createAccount(), n.actions.transfer(fund), n.actions.addFullAccessKey(n.PublicKey.fromString(deployKey.public_key))],
      `create \`${FACTORY}\` with ${fmtNear(fund)} NEAR and a temporary deploy key`,
    );
    await must(
      "deploy_factory",
      "factory_deploy",
      FACTORY,
      [
        n.actions.deployContract(new Uint8Array(factoryWasm)),
        fc("new", { owner: OWNER.account_id, fee_recipient: OWNER.account_id, default_tokens: [USDC, WRAP] }, 80),
        n.actions.deleteKey(n.PublicKey.fromString(deployKey.public_key)),
      ],
      `deploy the factory (${factoryWasm.length} bytes) and call new; the temporary key is deleted in the same batch`,
    );
  } else if (!done("deploy_factory")) {
    record({ step: "deploy_factory", summary: `\`${FACTORY}\` already exists; I left its code in place` });
  }

  const quote = await view(FACTORY, "get_create_cost", {});
  const quoted = BigInt(quote.total);
  if (quoted !== createCostYocto() && !done("create_wallet")) {
    say(`On-chain create cost is ${fmtNear(quoted)} NEAR. This build quotes ${fmtNear(createCostYocto())} NEAR.`);
  }
  const deposit = quoted > 0n ? quoted : createCostYocto();
  if ((await yoctoBalance(OWNER.account_id)) < deposit + 300_000_000_000_000_000_000_000n && !done("create_wallet")) {
    fail("not enough liquid NEAR left to attach the create deposit plus prepaid gas");
  }

  const name = walletName();
  const wallet = `${name}.${FACTORY}`;
  ev.wallet = wallet;
  save();
  await must(
    "create_wallet",
    "owner",
    FACTORY,
    [fc("create_wallet", { name }, 300, deposit)],
    `create \`${wallet}\`. Deposit ${fmtNear(deposit)} NEAR: 1 NEAR fee back to the owner, the rest is the wallet's code, state, and working balance`,
  );

  const payee = `gateway.${OWNER.account_id}`;
  if (!(await account(payee))) fail(`payee ${payee} does not exist`);
  const agentId = `tp-e2e-agent.${OWNER.account_id}`;
  const agentKey = newKey("agent_pay", agentId);
  if (!(await account(agentId))) {
    await must(
      "create_agent",
      "owner",
      agentId,
      [
        n.actions.createAccount(),
        n.actions.transfer(AGENT_NEAR),
        n.actions.addFunctionCallAccessKey(n.PublicKey.fromString(agentKey.public_key), wallet, ["pay"], AGENT_ALLOWANCE),
      ],
      `create \`${agentId}\` with one key: function-call, receiver the wallet, method pay, allowance 0.30 NEAR`,
    );
  }

  const registered = async (token, id) => (await view(token, "storage_balance_of", { account_id: id })) !== null;
  for (const token of [USDC, WRAP]) {
    for (const id of [wallet, payee]) {
      const step = `storage_${token.slice(0, 8)}_${id.split(".")[0]}`;
      if (await registered(token, id)) continue;
      await must(step, "owner", token, [fc("storage_deposit", { account_id: id }, 20, STORAGE_DEPOSIT)], `register \`${id}\` on \`${token}\``);
    }
  }
  if ((await ftBalance(WRAP, OWNER.account_id)) < WNEAR_AMOUNT) {
    await must("wrap_near", "owner", WRAP, [fc("near_deposit", {}, 30, WNEAR_AMOUNT * 2n)], "wrap a little NEAR so the wallet can hold wNEAR");
  }
  const usdc = await ftBalance(USDC, OWNER.account_id);
  if (usdc < USDC_AMOUNT) fail(`owner has ${usdc} atomic USDC, the flow sends ${USDC_AMOUNT}`);
  await must("fund_usdc", "owner", USDC, [fc("ft_transfer_call", { receiver_id: wallet, amount: USDC_AMOUNT.toString(), msg: "" }, 80, ONE)], `send 0.01 USDC to \`${wallet}\``);
  await must("fund_wnear", "owner", WRAP, [fc("ft_transfer_call", { receiver_id: wallet, amount: WNEAR_AMOUNT.toString(), msg: "" }, 80, ONE)], `send 1e-6 wNEAR to \`${wallet}\``);

  await must(
    "set_agent",
    "owner",
    wallet,
    [fc("set_agent", { agent_id: agentId, per_call_max: "0", daily_max: "0", payees: [payee] }, 40, ONE)],
    "set the payee allowlist. Limits stay per token, because USDC and wNEAR do not share a scale",
  );
  await must(
    "limits_usdc",
    "owner",
    wallet,
    [fc("set_token_limits", { agent_id: agentId, token: USDC, per_call_max: "50000", daily_max: "200000" }, 40, ONE)],
    "USDC limits: 0.05 per call, 0.20 per day",
  );
  await must(
    "limits_wnear",
    "owner",
    wallet,
    [fc("set_token_limits", { agent_id: agentId, token: WRAP, per_call_max: WNEAR_AMOUNT.toString(), daily_max: (WNEAR_AMOUNT * 4n).toString() }, 40, ONE)],
    "wNEAR limits cover the small payment four times",
  );

  const pay = (receiver, token, amount) => send("agent_pay", wallet, [fc("pay", { token, receiver_id: receiver, amount: amount.toString(), memo: null }, 80)]);
  if (!done("paid_usdc")) {
    const s = await pay(payee, USDC, 1_000n);
    if (s.outcome !== "success") fail("allowlisted USDC pay failed: " + (s.error_code || s.outcome));
    record({ step: "paid_usdc", ...s, summary: `allowlisted 0.001 USDC to \`${payee}\`: ${s.events.join(", ") || s.outcome}` });
  }
  if (!done("held_unlisted")) {
    const stranger = `nobody.${OWNER.account_id}`;
    const s = await pay(stranger, USDC, 1n);
    record({ step: "held_unlisted", ...s, summary: `unlisted payee: ${s.outcome} ${s.error_code || ""} ${s.events.join(", ")}` });
    if (s.outcome !== "success") fail("an unlisted payee should be held, not rejected by the transaction");
  }
  for (let i = 0; i < 3; i++) {
    const step = `hold_${i}`;
    if (done(step)) continue;
    const s = await pay(`junk${i}.${OWNER.account_id}`, USDC, 1n);
    if (s.outcome !== "success") fail(step + " failed");
    record({ step, ...s, summary: `queued hold ${i + 1} for an unlisted account` });
  }
  if (!done("fourth_hold_refused")) {
    const s = await pay(`junk3.${OWNER.account_id}`, USDC, 1n);
    record({ step: "fourth_hold_refused", ...s, summary: `fourth open hold: ${s.outcome} ${s.error_code || "no code in the logs"}` });
    if (s.outcome === "success" && !s.error_code) fail("the fourth hold was accepted");
  }
  if (!done("paid_while_holding")) {
    const s = await pay(payee, USDC, 1_000n);
    if (s.outcome !== "success") fail("an allowlisted payment was blocked by holds");
    record({ step: "paid_while_holding", ...s, summary: `allowlisted payment settled while junk holds were open: ${s.events.join(", ")}` });
  }
  await must("revoke", "owner", wallet, [fc("revoke_agent", { agent_id: agentId }, 30, ONE)], "revoke the agent");
  if (!done("revoked_refused")) {
    const s = await pay(payee, USDC, 1n);
    record({ step: "revoked_refused", ...s, summary: `revoked key: ${s.outcome} ${s.error_code || ""}` });
  }

  const usdcLeft = await ftBalance(USDC, wallet);
  if (usdcLeft > 0n) {
    await must("withdraw_usdc", "owner", wallet, [fc("withdraw", { token: USDC, amount: usdcLeft.toString(), to: OWNER.account_id }, 80, ONE)], `withdraw ${usdcLeft} atomic USDC`);
  }
  const nearLeft = await ftBalance(WRAP, wallet);
  if (nearLeft > 0n) {
    await must("withdraw_wnear", "owner", wallet, [fc("withdraw", { token: WRAP, amount: nearLeft.toString(), to: OWNER.account_id }, 80, ONE)], `withdraw ${nearLeft} yocto-wNEAR`);
  }
  await must("pause", "owner", wallet, [fc("pause", {}, 20, ONE)], "pause the wallet so it can be closed");
  const beforeClose = await yoctoBalance(OWNER.account_id);
  await must("close", "owner", wallet, [fc("close", { beneficiary: OWNER.account_id }, 80, ONE)], "delete the wallet. The code stake, the state, and the working balance return to the owner");
  const afterClose = await yoctoBalance(OWNER.account_id);
  record({
    step: "close_recovered",
    summary: `owner balance ${fmtNear(beforeClose)} -> ${fmtNear(afterClose)} NEAR. Fee paid on this run was ${fmtNear(FEE_YOCTO)} and came back because the owner is the fee recipient. A second wallet was not created.`,
  });
  say("\nDone. The factory account still holds its code stake. Deleting that account is a separate decision.");
}

main().catch((err) => fail(redact(err && err.stack ? err.stack : err)));
