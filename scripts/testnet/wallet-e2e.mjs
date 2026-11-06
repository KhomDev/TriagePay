// Testnet end-to-end run for the policy wallet (zero value).
//
// Testnet only, throwaway accounts generated for the run. Secret keys are
// written to the keys directory (outside the repository; the script refuses
// a path inside it) and are never printed or written to the evidence file.
// Evidence (tx hashes, outcomes, events, balance deltas, expectations) goes
// to the evidence file. The run is resumable: completed steps are recorded
// in the evidence file and skipped on a rerun.
//
// Usage (from the repository root, after `npm ci --prefix scripts/testnet`
// or with near-api-js 7.3.1 resolvable from this directory):
//
//   node scripts/testnet/wallet-e2e.mjs [--network testnet]
//        [--keys-dir DIR] [--evidence FILE] [--wasm FILE]
//        [--expected-sha256 HEX]
//
// Environment equivalents: APS_WALLET_KEYS_DIR, APS_WALLET_EVIDENCE,
// APS_WALLET_WASM, APS_WALLET_WASM_SHA256, and APS_TESTNET_E2E_DIR for the
// off-repository folder that holds the default keys directory
// (`wallet-keys/`) and evidence file (`wallet-EVIDENCE.json`). Without
// APS_TESTNET_E2E_DIR the script uses the first `testnet-e2e` folder found
// beside the repository or beside one of its parent folders.
//
// Exit codes: 0 done, 1 unexpected error, 2 an expectation failed or a
// precondition is missing, 3 the testnet faucet refused the request.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as n from "near-api-js";

const RPC = "https://rpc.testnet.near.org";
const FAUCET = "https://faucet-near-testnet.onrender.com/api/faucet";
const FAUCET_DOCS = "https://docs.near.org/getting-started/faucet";
const EXPLORER = "https://testnet.nearblocks.io/txns/";
const REGISTRAR = "testnet";
const WRAP = "wrap.testnet";
const YOCTO_PER_BYTE = 10_000_000_000_000_000_000n; // storage_amount_per_byte, 1e19

// ----- configuration ---------------------------------------------------------
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..", "..");

function arg(name) {
  const i = process.argv.indexOf(name);
  return i > 0 && i + 1 < process.argv.length ? process.argv[i + 1] : null;
}

function findOffRepoDir() {
  if (process.env.APS_TESTNET_E2E_DIR) return path.resolve(process.env.APS_TESTNET_E2E_DIR);
  let dir = path.dirname(REPO_ROOT);
  for (;;) {
    const candidate = path.join(dir, "testnet-e2e");
    if (fs.existsSync(candidate) && fs.statSync(candidate).isDirectory()) return candidate;
    const parent = path.dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

function insideRepo(p) {
  const rel = path.relative(REPO_ROOT, path.resolve(p));
  return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
}

const network = arg("--network") || "testnet";
if (network !== "testnet") {
  console.log("STOP: this script runs on testnet only (got --network " + network + ")");
  process.exit(2);
}
const offRepo = findOffRepoDir();
const KEYS_DIR = path.resolve(arg("--keys-dir") || process.env.APS_WALLET_KEYS_DIR || (offRepo && path.join(offRepo, "wallet-keys")) || "");
const EVIDENCE = path.resolve(arg("--evidence") || process.env.APS_WALLET_EVIDENCE || (offRepo && path.join(offRepo, "wallet-EVIDENCE.json")) || "");
const WASM = path.resolve(arg("--wasm") || process.env.APS_WALLET_WASM || path.join(REPO_ROOT, "contracts", "target", "near", "policy_wallet", "policy_wallet.wasm"));
const EXPECTED_SHA256 = (arg("--expected-sha256") || process.env.APS_WALLET_WASM_SHA256 || "").toLowerCase() || null;

if (!offRepo && !(arg("--keys-dir") || process.env.APS_WALLET_KEYS_DIR)) {
  console.log("STOP: no keys directory; pass --keys-dir or set APS_TESTNET_E2E_DIR");
  process.exit(2);
}
for (const [label, p] of [
  ["keys directory", KEYS_DIR],
  ["evidence file", EVIDENCE],
]) {
  if (insideRepo(p)) {
    console.log("STOP: the " + label + " must be outside the repository");
    process.exit(2);
  }
}

const T = (tgas) => BigInt(tgas) * 1_000_000_000_000n;
const NEAR = (s) => BigInt(n.parseNearAmount(s));
const YOCTO1 = 1n;
// wNEAR has 24 decimals. Limits are tiny: this is a zero-value run.
const PER_CALL = 1_000_000_000_000_000_000_000n; // 0.001 wNEAR
const DAILY = 3_000_000_000_000_000_000_000n; // 0.003 wNEAR
const FUNDING = 10_000_000_000_000_000_000_000n; // 0.01 wNEAR
const ALLOWANCE = NEAR("0.25"); // five pay calls at 50 TGas x 0.001 NEAR/TGas
const WALLET_HEADROOM = NEAR("0.10"); // state and receipts beyond the code stake

const provider = new n.JsonRpcProvider({ url: RPC });

// ----- evidence --------------------------------------------------------------
function freshEvidence() {
  return {
    run: "policy-wallet testnet e2e",
    network: "testnet",
    rpc: RPC,
    explorer: EXPLORER,
    value: "zero (testnet tokens only)",
    started_at: new Date().toISOString(),
    tooling: "near-api-js 7.3.1, Node " + process.version,
    funding_source: null,
    wasm: null,
    accounts: {},
    steps: [],
  };
}
let ev = fs.existsSync(EVIDENCE) ? JSON.parse(fs.readFileSync(EVIDENCE, "utf8")) : freshEvidence();
function save() {
  fs.mkdirSync(path.dirname(EVIDENCE), { recursive: true });
  fs.writeFileSync(EVIDENCE, JSON.stringify(ev, null, 2) + "\n");
}
function done(name) {
  return ev.steps.find((s) => s.step === name);
}
function record(entry) {
  ev.steps.push({ at: new Date().toISOString(), ...entry });
  save();
  console.log(JSON.stringify({ step: entry.step, tx_hash: entry.tx_hash, outcome: entry.outcome, code: entry.error_code, pass: entry.pass }));
}
function stop(msg, code) {
  ev.stopped = msg;
  ev.stopped_at = new Date().toISOString();
  save();
  console.log("STOP: " + msg);
  process.exit(code || 2);
}

// ----- keys (never printed, never in the evidence) ---------------------------
function keyFile(role) {
  return path.join(KEYS_DIR, role + ".json");
}
function newKey(role, accountId) {
  fs.mkdirSync(KEYS_DIR, { recursive: true });
  const f = keyFile(role);
  if (fs.existsSync(f)) return JSON.parse(fs.readFileSync(f, "utf8"));
  const kp = n.KeyPair.fromRandom("ed25519");
  const k = { role, account_id: accountId, public_key: kp.getPublicKey().toString(), secret_key: kp.toString() };
  fs.writeFileSync(f, JSON.stringify(k, null, 2) + "\n", { mode: 0o600 });
  return k;
}
function loadKey(role) {
  return JSON.parse(fs.readFileSync(keyFile(role), "utf8"));
}
function accountFor(role) {
  const k = loadKey(role);
  return new n.Account(k.account_id, provider, k.secret_key);
}
const redact = (s) => String(s).replace(/ed25519:[1-9A-HJ-NP-Za-km-z]{30,}/g, "ed25519:<redacted>");

// ----- RPC helpers -----------------------------------------------------------
async function rpc(method, params) {
  const r = await fetch(RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "e2e", method, params }),
  });
  return r.json();
}
async function viewAccount(id) {
  const r = await rpc("query", { request_type: "view_account", finality: "final", account_id: id });
  if (r.error || (r.result && r.result.error)) return null;
  return r.result;
}
async function viewCall(contract, method, args) {
  const r = await rpc("query", {
    request_type: "call_function",
    finality: "final",
    account_id: contract,
    method_name: method,
    args_base64: Buffer.from(JSON.stringify(args)).toString("base64"),
  });
  if (r.error || r.result.error) throw new Error("view failed: " + JSON.stringify(r.error || r.result.error).slice(0, 300));
  return JSON.parse(Buffer.from(r.result.result).toString("utf8"));
}
async function accessKeyCount(id) {
  const r = await rpc("query", { request_type: "view_access_key_list", finality: "final", account_id: id });
  return r.result ? r.result.keys.length : null;
}
async function ftBalance(id) {
  return BigInt(await viewCall(WRAP, "ft_balance_of", { account_id: id }));
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function base58(buf) {
  let x = BigInt("0x" + Buffer.from(buf).toString("hex"));
  let out = "";
  while (x > 0n) {
    out = B58[Number(x % 58n)] + out;
    x /= 58n;
  }
  for (const b of buf) {
    if (b !== 0) break;
    out = "1" + out;
  }
  return out;
}

// ----- transactions ----------------------------------------------------------
function summarize(res) {
  const all = JSON.stringify(res);
  const code = (all.match(/ERR_[A-Z_]+/) || [null])[0];
  const yocto = all.includes("Requires attached deposit of exactly 1 yoctoNEAR");
  const outcomes = res.receipts_outcome || [];
  const events = outcomes
    .flatMap((o) => o.outcome.logs)
    .filter((l) => l.startsWith("EVENT_JSON:"))
    .map((l) => {
      try {
        return JSON.parse(l.slice(11)).event;
      } catch (_) {
        return "unparsed";
      }
    });
  const failed = !!(res.status && res.status.Failure);
  return {
    tx_hash: res.transaction.hash,
    outcome: failed ? "failure" : "success",
    error_code: code || (yocto ? "assert_one_yocto" : null),
    receipt_failures: outcomes.filter((o) => o.outcome.status && o.outcome.status.Failure).length,
    events,
    gas_burnt: String(outcomes.reduce((a, o) => a + BigInt(o.outcome.gas_burnt), BigInt(res.transaction_outcome.outcome.gas_burnt))),
  };
}

async function send(role, receiverId, actions) {
  return accountFor(role).signAndSendTransaction({ receiverId, actions, waitUntil: "FINAL", throwOnFailure: false });
}

// expect: { outcome, code?, event?, check?(entry) -> bool }
function judge(entry, expect) {
  if (!expect) return null;
  if (expect.outcome && entry.outcome !== expect.outcome) return false;
  if (expect.code && entry.error_code !== expect.code) return false;
  if (expect.event && !(entry.events || []).includes(expect.event)) return false;
  if (expect.check && !expect.check(entry)) return false;
  return true;
}

async function step(name, role, receiverId, actionsFn, description, expect, extra) {
  if (done(name)) return done(name);
  const res = await send(role, receiverId, actionsFn());
  const s = summarize(res);
  const entry = { step: name, description, signer: loadKey(role).account_id, receiver: receiverId, ...s };
  if (extra) Object.assign(entry, await extra(s));
  entry.expected = expect ? expect.text : null;
  entry.pass = judge(entry, expect);
  record(entry);
  if (entry.pass === false) stop("expectation failed at step " + name);
  return entry;
}

// A transaction the node must reject before execution (no tx hash).
async function rejected(name, role, receiverId, actionsFn, description) {
  if (done(name)) return done(name);
  let entry;
  try {
    const res = await send(role, receiverId, actionsFn());
    entry = { step: name, description, signer: loadKey(role).account_id, receiver: receiverId, ...summarize(res), node_rejected: false };
  } catch (e) {
    entry = {
      step: name,
      description,
      signer: loadKey(role).account_id,
      receiver: receiverId,
      tx_hash: null,
      outcome: "rejected_by_node",
      node_rejected: true,
      error: redact((e && (e.type || e.message)) || e).slice(0, 300),
    };
  }
  entry.expected = "rejected by the node before execution";
  entry.pass = entry.node_rejected === true;
  record(entry);
  if (!entry.pass) stop("expectation failed at step " + name);
  return entry;
}

const fc = (method, args, tgas, deposit) => n.actions.functionCall(method, args, T(tgas), deposit || 0n);

// ----- run ---------------------------------------------------------------------
try {
  // 0. Wasm check.
  if (!fs.existsSync(WASM)) stop("wasm not found; build it with cargo near first (docs/contracts/build.md)");
  const wasm = fs.readFileSync(WASM);
  const sha = crypto.createHash("sha256").update(wasm).digest("hex");
  if (EXPECTED_SHA256 && sha !== EXPECTED_SHA256) stop("wasm hash mismatch: " + sha);
  // A deployed wasm is public: refuse one that embeds a host path (build with
  // the path remapping in docs/contracts/build.md).
  if (/[A-Za-z]:\\|\/Users\/|\/home\//.test(wasm.toString("latin1"))) stop("the wasm embeds a host path; rebuild it with the path remapping in docs/contracts/build.md");
  if (ev.wasm && ev.wasm.sha256 !== sha && ev.steps.length > 0) stop("the evidence file belongs to a run with another wasm; pass a new --evidence file");
  ev.wasm = { sha256: sha, sha256_base58: base58(crypto.createHash("sha256").update(wasm).digest()), bytes: wasm.length };
  save();
  const codeStake = BigInt(wasm.length) * YOCTO_PER_BYTE;

  // 1. Funder: an implicit account funded by the official testnet faucet.
  const tag = ev.tag || (ev.tag = crypto.randomBytes(3).toString("hex"));
  const funderKey = newKey("funder", null);
  const funderId = n.keyToImplicitAddress(funderKey.public_key);
  if (!funderKey.account_id) {
    funderKey.account_id = funderId;
    fs.writeFileSync(keyFile("funder"), JSON.stringify(funderKey, null, 2) + "\n", { mode: 0o600 });
  }
  ev.accounts.funder = funderId;
  delete ev.stopped;
  delete ev.stopped_at;
  save();
  const funded = (f) => f && f.http_status >= 200 && f.http_status < 300;
  if (!funded(ev.funding_source) && !(await viewAccount(funderId))) {
    if (ev.funding_source) (ev.faucet_attempts = ev.faucet_attempts || []).push(ev.funding_source);
    let status = null;
    let body = null;
    try {
      const r = await fetch(FAUCET, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ accountId: funderId }) });
      status = r.status;
      body = (await r.text()).slice(0, 400);
    } catch (e) {
      ev.funding_source = { at: new Date().toISOString(), faucet: FAUCET, docs: FAUCET_DOCS, http_status: null, response: String(e.message || e).slice(0, 200) };
      stop("faucet unreachable", 3);
    }
    ev.funding_source = { at: new Date().toISOString(), faucet: FAUCET, docs: FAUCET_DOCS, http_status: status, response: body };
    save();
    if (status < 200 || status >= 300) stop("faucet refused the request (HTTP " + status + ")", 3);
  }
  let funder = null;
  for (let i = 0; i < 30 && !funder; i++) {
    funder = await viewAccount(funderId);
    if (!funder) await sleep(4000);
  }
  if (!funder) stop("funder account not visible after the faucet call");
  ev.funding_source.funder_balance_yocto = ev.funding_source.funder_balance_yocto || funder.amount;
  save();

  // 2. Named throwaway accounts through the testnet registrar.
  const walletFunding = codeStake + WALLET_HEADROOM;
  const plan = [
    ["owner", NEAR("0.6")],
    ["agent", NEAR("0.4")],
    ["payee", NEAR("0.05")],
    ["wallet", walletFunding],
  ];
  const need = plan.reduce((a, [, v]) => a + v, 0n) + NEAR("0.2");
  if (!done("create_owner") && BigInt(funder.amount) < need) stop("faucet funding " + n.formatNearAmount(funder.amount, 4) + " NEAR is below the " + n.formatNearAmount(need.toString(), 4) + " NEAR this run needs");
  for (const [role, amount] of plan) {
    const id = `apsw-${tag}-${role}.testnet`;
    const k = newKey(role, id);
    ev.accounts[role] = id;
    save();
    await step(
      `create_${role}`,
      "funder",
      REGISTRAR,
      () => [fc("create_account", { new_account_id: id, new_public_key: k.public_key }, 100, amount)],
      `create ${id} via the testnet registrar with ${n.formatNearAmount(amount.toString(), 5)} NEAR`,
      { text: "success; the account exists", outcome: "success", check: () => true },
      async () => ({ account_exists: !!(await viewAccount(id)) }),
    );
  }
  const A = ev.accounts;

  // 3. Deploy + new + delete its own signing key, one batch signed by the wallet account.
  const walletKey = loadKey("wallet");
  await step(
    "deploy_init_lock",
    "wallet",
    A.wallet,
    () => [n.actions.deployContract(new Uint8Array(wasm)), fc("new", { owner: A.owner, tokens: [WRAP] }, 30), n.actions.deleteKey(n.PublicKey.fromString(walletKey.public_key))],
    "DeployContract + FunctionCall new + DeleteKey (its own signing key) in one batch",
    {
      text: "success; the wallet has zero access keys; on-chain code hash equals the local wasm hash; get_config shows the owner and wrap.testnet",
      outcome: "success",
      check: (e) => e.after.access_key_count === 0 && e.after.code_hash_base58 === ev.wasm.sha256_base58 && e.after.config.owner === A.owner,
    },
    async () => {
      const acct = await viewAccount(A.wallet);
      const config = await viewCall(A.wallet, "get_config", {});
      return {
        after: {
          access_key_count: await accessKeyCount(A.wallet),
          code_hash_base58: acct && acct.code_hash,
          storage_usage: acct && acct.storage_usage,
          balance_yocto: acct && acct.amount,
          config,
        },
      };
    },
  );

  // 4. wNEAR: the owner registers and wraps; storage_deposit for the wallet and the payee.
  const ok = (text, event) => ({ text, outcome: "success", event });
  await step("owner_wrap", "owner", WRAP, () => [fc("storage_deposit", {}, 30, NEAR("0.00125")), fc("near_deposit", {}, 30, NEAR("0.05"))], "owner registers on wrap.testnet and wraps 0.05 NEAR", ok("success"));
  await step("storage_wallet", "owner", WRAP, () => [fc("storage_deposit", { account_id: A.wallet, registration_only: true }, 30, NEAR("0.00125"))], "storage_deposit for the wallet on wrap.testnet", ok("success"));
  await step("storage_payee", "owner", WRAP, () => [fc("storage_deposit", { account_id: A.payee, registration_only: true }, 30, NEAR("0.00125"))], "storage_deposit for the payee on wrap.testnet", ok("success"));

  // 5. Fund through ft_transfer_call.
  await step(
    "fund_wallet",
    "owner",
    WRAP,
    () => [fc("ft_transfer_call", { receiver_id: A.wallet, amount: FUNDING.toString(), msg: "" }, 100, YOCTO1)],
    "fund the wallet with 0.01 wNEAR via ft_transfer_call",
    { text: "success; funded event; wallet wNEAR balance equals the funding", outcome: "success", event: "funded", check: (e) => e.wallet_wnear === FUNDING.toString() },
    async () => ({ wallet_wnear: (await ftBalance(A.wallet)).toString() }),
  );

  // 6. Agent policy, then a pay-only function-call key on the agent account.
  await step(
    "set_agent",
    "owner",
    A.wallet,
    () => [fc("set_agent", { agent_id: A.agent, per_call_max: PER_CALL.toString(), daily_max: DAILY.toString(), payees: [A.payee] }, 30, YOCTO1)],
    "set_agent: per-call 0.001 wNEAR, daily 0.003 wNEAR, payees [payee]",
    ok("success; agent_set event", "agent_set"),
  );
  const fcKey = newKey("agent_fc", A.agent);
  await step(
    "add_agent_fc_key",
    "agent",
    A.agent,
    () => [n.actions.addFunctionCallAccessKey(n.PublicKey.fromString(fcKey.public_key), A.wallet, ["pay"], ALLOWANCE)],
    "AddKey function-call key: receiver wallet, methods [pay], allowance 0.25 NEAR",
    ok("success"),
  );

  const payArgs = (receiver, amount) => () => [fc("pay", { token: WRAP, receiver_id: receiver, amount: amount.toString(), memo: "e2e" }, 50)];

  // 7. Pay within the limit; assert the payee's balance delta.
  const payee0 = done("pay_within_limit") ? null : await ftBalance(A.payee);
  await step(
    "pay_within_limit",
    "agent_fc",
    A.wallet,
    payArgs(A.payee, PER_CALL),
    "agent pays 0.001 wNEAR to the payee with its function-call key",
    { text: "success; pay_settled; payee delta equals the amount", outcome: "success", event: "pay_settled", check: (e) => e.payee_delta === PER_CALL.toString() },
    async () => {
      const after = await ftBalance(A.payee);
      return { payee_before: payee0.toString(), payee_after: after.toString(), payee_delta: (after - payee0).toString(), expected_delta: PER_CALL.toString(), spent: await viewCall(A.wallet, "get_spent", { agent_id: A.agent }) };
    },
  );

  // 8. Refusals with stable codes; no balance moves.
  const unchanged = async () => ({ payee_wnear: (await ftBalance(A.payee)).toString(), wallet_wnear: (await ftBalance(A.wallet)).toString() });
  // After the one settled payment the wallet holds FUNDING - PER_CALL; a refusal must leave it there.
  const noMove = (e) => e.wallet_wnear === (FUNDING - PER_CALL).toString();
  await step("pay_over_per_call", "agent_fc", A.wallet, payArgs(A.payee, PER_CALL + 1n), "pay 0.001 wNEAR + 1 atomic unit", { text: "failure ERR_PER_CALL_LIMIT; wallet balance unchanged", outcome: "failure", code: "ERR_PER_CALL_LIMIT", check: noMove }, unchanged);
  await step("pay_non_allowlisted", "agent_fc", A.wallet, payArgs(A.owner, 1n), "pay to the owner (not on the payee list)", { text: "failure ERR_PAYEE_NOT_ALLOWED; wallet balance unchanged", outcome: "failure", code: "ERR_PAYEE_NOT_ALLOWED", check: noMove }, unchanged);

  // 9. Owner methods are unreachable from function-call keys.
  await rejected(
    "agent_fc_key_calls_set_agent",
    "agent_fc",
    A.wallet,
    () => [fc("set_agent", { agent_id: A.agent, per_call_max: DAILY.toString(), daily_max: DAILY.toString(), payees: [A.agent] }, 30, YOCTO1)],
    "agent function-call key calls set_agent with 1 yocto attached",
  );
  await step(
    "agent_full_key_set_agent",
    "agent",
    A.wallet,
    () => [fc("set_agent", { agent_id: A.agent, per_call_max: DAILY.toString(), daily_max: DAILY.toString(), payees: [A.agent] }, 30, YOCTO1)],
    "agent account's full-access key calls set_agent with 1 yocto",
    { text: "failure ERR_NOT_OWNER", outcome: "failure", code: "ERR_NOT_OWNER" },
  );
  // The owner's own function-call key (any method on the wallet) cannot reach owner methods either.
  const ownerFc = newKey("owner_fc", A.owner);
  await step("add_owner_fc_key", "owner", A.owner, () => [n.actions.addFunctionCallAccessKey(n.PublicKey.fromString(ownerFc.public_key), A.wallet, [], NEAR("0.1"))], "AddKey on the owner account: function-call key, receiver wallet, any method", ok("success"));
  await step("owner_fc_key_calls_pause", "owner_fc", A.wallet, () => [fc("pause", {}, 30)], "owner's function-call key calls pause with zero deposit", {
    text: "failure assert_one_yocto (Requires attached deposit of exactly 1 yoctoNEAR)",
    outcome: "failure",
    code: "assert_one_yocto",
  });
  await rejected("owner_fc_key_pause_with_yocto", "owner_fc", A.wallet, () => [fc("pause", {}, 30, YOCTO1)], "owner's function-call key calls pause with 1 yocto attached");
  await step("delete_owner_fc_key", "owner", A.owner, () => [n.actions.deleteKey(n.PublicKey.fromString(ownerFc.public_key))], "owner deletes the function-call key", ok("success"));

  // 10. Revoke the agent, pay again (refused), delete the key, pay again (rejected by the node).
  await step("revoke_agent", "owner", A.wallet, () => [fc("revoke_agent", { agent_id: A.agent }, 30, YOCTO1)], "owner revokes the agent", ok("success; agent_revoked", "agent_revoked"));
  await step("pay_after_revoke", "agent_fc", A.wallet, payArgs(A.payee, 1n), "revoked agent pays with its still-present function-call key", { text: "failure ERR_AGENT_INACTIVE; wallet balance unchanged", outcome: "failure", code: "ERR_AGENT_INACTIVE", check: noMove }, unchanged);
  await step("delete_agent_fc_key", "agent", A.agent, () => [n.actions.deleteKey(n.PublicKey.fromString(fcKey.public_key))], "agent account deletes the function-call key", ok("success"));
  await rejected("pay_with_deleted_key", "agent_fc", A.wallet, payArgs(A.payee, 1n), "pay with the deleted function-call key");

  // 11. Pause, close refused while funded, withdraw everything, close.
  await step("pause", "owner", A.wallet, () => [fc("pause", {}, 30, YOCTO1)], "owner pauses the wallet", ok("success; paused", "paused"));
  await step(
    "close_refused_with_balance",
    "owner",
    A.wallet,
    () => [fc("close", { beneficiary: A.owner }, 50, YOCTO1)],
    "close while wNEAR remains",
    { text: "success with close_refused; the wallet still exists", outcome: "success", event: "close_refused", check: (e) => e.wallet_exists === true },
    async () => ({ wallet_exists: !!(await viewAccount(A.wallet)) }),
  );
  const remaining = done("withdraw_all") ? null : await ftBalance(A.wallet);
  await step(
    "withdraw_all",
    "owner",
    A.wallet,
    () => [fc("withdraw", { token: WRAP, amount: remaining.toString(), to: A.owner }, 50, YOCTO1)],
    "owner withdraws the full wNEAR balance",
    { text: "success; withdrawn; wallet wNEAR is zero", outcome: "success", event: "withdrawn", check: (e) => e.wallet_wnear_after === "0" },
    async () => ({ withdrawn: remaining.toString(), wallet_wnear_after: (await ftBalance(A.wallet)).toString() }),
  );
  let before = null;
  if (!done("close")) {
    const w = await viewAccount(A.wallet);
    const o = await viewAccount(A.owner);
    before = { wallet_near: w.amount, wallet_storage_usage: w.storage_usage, owner_near: o.amount };
  }
  await step(
    "close",
    "owner",
    A.wallet,
    () => [fc("close", { beneficiary: A.owner }, 50, YOCTO1)],
    "close(owner) with every allowlisted balance at zero",
    {
      text: "success; closed; the wallet account no longer exists; the owner's NEAR delta covers the storage stake",
      outcome: "success",
      event: "closed",
      check: (e) => e.wallet_exists_after === false && BigInt(e.owner_near_delta) >= BigInt(e.storage_stake_at_close),
    },
    async () => {
      await sleep(3000);
      const w = await viewAccount(A.wallet);
      const o = await viewAccount(A.owner);
      const gained = BigInt(o.amount) - BigInt(before.owner_near);
      const stake = BigInt(before.wallet_storage_usage) * YOCTO_PER_BYTE;
      return {
        wallet_exists_after: !!w,
        before,
        owner_near_after: o.amount,
        owner_near_delta: gained.toString(),
        storage_stake_at_close: stake.toString(),
        delta_minus_stake: (gained - stake).toString(),
        note: "owner delta = the wallet's whole NEAR balance (storage stake plus headroom) returned by the deletion, minus the owner's gas for this transaction",
      };
    },
  );

  ev.finished_at = new Date().toISOString();
  ev.all_pass = ev.steps.every((s) => s.pass !== false);
  save();
  console.log("DONE all_pass=" + ev.all_pass);
} catch (e) {
  const msg = redact((e && (e.stack || e.message)) || e);
  ev.error = msg.slice(0, 600);
  save();
  console.log("ERR " + msg.slice(0, 600));
  process.exit(1);
}
