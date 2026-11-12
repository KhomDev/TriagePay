// One-time NEAR testnet setup for the hosted demo.
//
// Creates a faucet-funded implicit account, then owner, agent, payee,
// flagged, and wallet accounts. Deploys the policy wallet (no access keys
// left on it), funds it with wNEAR, sets the agent policy, and adds a
// function-call key that can call only `pay`.
//
// Keys are written to demo/live/.keys (gitignored). config.json receives
// account ids and creation transactions only.
//
//   node setup.mjs
//   TRIAGEPAY_WALLET_WASM=path/to/policy_wallet.wasm node setup.mjs

import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { assertPublicConfig } from "./lib/config.js";
import { redact } from "./lib/http.js";
import { selectKeysToDelete } from "./lib/keys.js";
import {
  addPayKey,
  assertTestnet,
  deleteKey,
  deployContract,
  functionCall,
  implicitAccountId,
  parseNear,
  randomKeyPair,
  signCall,
  viewAccessKeys,
  viewAccount,
  viewCall,
} from "./lib/near.js";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const KEYS = join(ROOT, ".keys");
const STATE_PATH = join(KEYS, "state.json");
const RPC = process.env.TRIAGEPAY_RPC_URL || "https://test.rpc.fastnear.com";
const FAUCETS = [
  "https://faucet-near-testnet.onrender.com/api/faucet",
];
const REGISTRAR = "testnet";
const TOKEN = "wrap.testnet";
const PER_CALL = 10n ** 21n;
const DAILY = 20n * PER_CALL;
const ALLOWED = 10n ** 20n;
const FUNDING = 5n * 10n ** 22n;
const YOCTO_PER_BYTE = 10n ** 19n;

function fail(message, code = 1) {
  console.error("STOP: " + redact(message));
  process.exit(code);
}

mkdirSync(KEYS, { recursive: true });
let state = existsSync(STATE_PATH)
  ? JSON.parse(readFileSync(STATE_PATH, "utf8"))
  : { tag: null, accounts: {}, creationTx: {}, steps: [] };

function save() {
  writeFileSync(STATE_PATH, JSON.stringify(state, null, 2) + "\n", { mode: 0o600 });
}

function keyPath(role) {
  return join(KEYS, role + ".json");
}

function loadKey(role) {
  return JSON.parse(readFileSync(keyPath(role), "utf8"));
}

function writeKey(key) {
  const path = keyPath(key.role);
  writeFileSync(path, JSON.stringify(key, null, 2) + "\n", { mode: 0o600 });
  chmodSync(path, 0o600);
  return key;
}

function newKey(role, accountId) {
  if (existsSync(keyPath(role))) {
    const existing = loadKey(role);
    if (accountId && !existing.account_id) {
      existing.account_id = accountId;
      return writeKey(existing);
    }
    return existing;
  }
  const pair = randomKeyPair();
  return writeKey({ role, account_id: accountId || null, public_key: pair.public_key, secret_key: pair.secret_key });
}

const done = (name) => state.steps.find((step) => step.name === name);

async function must(name, role, receiverId, actions) {
  const existing = done(name);
  if (existing) return existing;
  const key = loadKey(role);
  const result = await signCall({
    rpcUrl: RPC,
    signerId: key.account_id,
    secretKey: key.secret_key,
    receiverId,
    actions,
    waitUntil: "EXECUTED",
  });
  if (result.rejected || result.outcome !== "success") {
    fail(`${name} failed (${result.errorCode || result.detail || result.outcome})`);
  }
  const entry = { name, txHash: result.txHash, signer: key.account_id };
  state.steps.push(entry);
  save();
  console.log(`${name} https://testnet.nearblocks.io/txns/${result.txHash}`);
  return entry;
}

function wasmPath() {
  const candidates = [
    process.env.TRIAGEPAY_WALLET_WASM,
    join(REPO, "contracts", "target", "wasm32-unknown-unknown", "release", "policy_wallet.wasm"),
    join(REPO, "contracts", "target", "near", "policy_wallet", "policy_wallet.wasm"),
  ].filter(Boolean);
  return candidates.find((path) => existsSync(path)) || null;
}

async function fund(accountId) {
  let last = "no response";
  for (const url of FAUCETS) {
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ accountId }),
        signal: AbortSignal.timeout(90000),
      });
      const text = (await response.text()).slice(0, 300);
      console.log(`faucet ${url} HTTP ${response.status} ${text}`);
      if (response.status >= 200 && response.status < 300) return;
      last = `HTTP ${response.status} ${text}`;
    } catch (error) {
      last = redact(error && error.message ? error.message : error);
      console.log(`faucet ${url} failed: ${last}`);
    }
  }
  fail("faucet refused the request (" + last + ")", 3);
}

async function waitForAccount(accountId) {
  for (let i = 0; i < 30; i++) {
    const account = await viewAccount(RPC, accountId);
    if (account) return account;
    await new Promise((resolve) => setTimeout(resolve, 4000));
  }
  return null;
}

async function main() {
  await assertTestnet(RPC);
  const wasmFile = wasmPath();
  if (!wasmFile) fail("policy wallet wasm not found. Build it, or set TRIAGEPAY_WALLET_WASM.");
  const wasm = readFileSync(wasmFile);
  if (/[A-Za-z]:\\|\/Users\/|\/home\//.test(Buffer.from(wasm).toString("latin1"))) {
    fail("the wasm embeds a host path; rebuild it with path remapping");
  }
  if (!Buffer.from(wasm).includes("panic_utf8")) {
    fail("the wasm has no panic_utf8 import. Build with RUSTFLAGS='--cfg near' (cargo-near adds this). Without it, refusals are silent traps.");
  }
  const sha256 = createHash("sha256").update(wasm).digest("hex");
  console.log(`wasm ${wasm.length} bytes sha256 ${sha256}`);

  if (!state.tag) {
    state.tag = createHash("sha256").update(String(Date.now()) + Math.random()).digest("hex").slice(0, 6);
    save();
  }
  const tag = state.tag;
  const idFor = (role) => `tpd-${tag}-${role}.testnet`;

  const funder = newKey("funder", null);
  if (!funder.account_id) {
    funder.account_id = implicitAccountId(funder.public_key);
    writeKey(funder);
  }
  state.accounts.funder = funder.account_id;
  save();

  let funderAccount = await viewAccount(RPC, funder.account_id);
  if (!funderAccount) {
    console.log(`requesting faucet funds for ${funder.account_id}`);
    await fund(funder.account_id);
    funderAccount = await waitForAccount(funder.account_id);
  }
  if (!funderAccount) fail("funder account was not visible after the faucet call", 3);
  console.log(`funder ${funder.account_id} balance ${funderAccount.amount}`);

  const codeStake = BigInt(wasm.length) * YOCTO_PER_BYTE;
  const plan = {
    owner: parseNear("1"),
    agent: parseNear("0.8"),
    payee: parseNear("0.05"),
    flagged: parseNear("0.05"),
    wallet: codeStake + parseNear("0.2"),
  };
  const need = Object.values(plan).reduce((sum, value) => sum + value, 0n) + parseNear("0.3");
  if (!done("create_owner") && BigInt(funderAccount.amount) < need) {
    fail(`faucet balance is below the ${need.toString()} yoctoNEAR this setup needs`, 3);
  }

  for (const role of ["owner", "agent", "payee", "flagged", "wallet"]) {
    const accountId = idFor(role);
    state.accounts[role] = accountId;
    newKey(role, accountId);
    save();
    const key = loadKey(role);
    const step = await must("create_" + role, "funder", REGISTRAR, [
      await functionCall("create_account", { new_account_id: accountId, new_public_key: key.public_key }, 100, plan[role]),
    ]);
    state.creationTx[accountId] = { hash: step.txHash, signer: funder.account_id };
    save();
  }

  const accounts = state.accounts;
  const walletKey = loadKey("wallet");
  await must("deploy_wallet", "wallet", accounts.wallet, [
    deployContract(new Uint8Array(wasm)),
    await functionCall("new", { owner: accounts.owner, tokens: [TOKEN] }, 40, 0n),
    deleteKey(walletKey.public_key),
  ]);

  await must("wrap_owner", "owner", TOKEN, [
    await functionCall("storage_deposit", {}, 30, parseNear("0.00125")),
    await functionCall("near_deposit", {}, 40, FUNDING),
  ]);
  await must("register_payee", "owner", TOKEN, [
    await functionCall("storage_deposit", { account_id: accounts.wallet, registration_only: true }, 30, parseNear("0.00125")),
    await functionCall("storage_deposit", { account_id: accounts.payee, registration_only: true }, 30, parseNear("0.00125")),
  ]);
  await must("fund_wallet", "owner", TOKEN, [
    await functionCall("ft_transfer_call", {
      receiver_id: accounts.wallet,
      amount: FUNDING.toString(),
      msg: "",
    }, 150, 1n),
  ]);
  await must("set_agent", "owner", accounts.wallet, [
    await functionCall("set_agent", {
      agent_id: accounts.agent,
      per_call_max: PER_CALL.toString(),
      daily_max: DAILY.toString(),
      payees: [accounts.payee],
    }, 30, 1n),
  ]);
  // Amounts above the small demo payment wait for the owner. Equal to the
  // allowed amount still pays. Unknown payees stay ERR_PAYEE_NOT_ALLOWED, so
  // the injected-payee attack is still a refusal on chain. Set
  // TRIAGEPAY_REVIEW_UNKNOWN_PAYEES=1 to queue them for the owner instead.
  await must("set_review", "owner", accounts.wallet, [
    await functionCall("set_review_policy", {
      approval_threshold: ALLOWED.toString(),
      review_unknown_payees: process.env.TRIAGEPAY_REVIEW_UNKNOWN_PAYEES === "1",
    }, 30, 1n),
  ]);

  // agent.json is the full-access key until this point. Move it aside, add a
// function-call key, then leave only that limited key in agent.json. The
// local server and Vercel both read agent.json and must not receive a key
// that can do more than pay.
  if (!existsSync(keyPath("agent-full"))) writeKey({ ...loadKey("agent"), role: "agent-full" });
  const fcKey = existsSync(keyPath("agent-fc"))
    ? loadKey("agent-fc")
    : writeKey({ role: "agent-fc", account_id: accounts.agent, ...randomKeyPair() });
  await must("add_agent_key", "agent-full", accounts.agent, [
    addPayKey(fcKey.public_key, accounts.wallet, parseNear("0.25")),
  ]);
  writeKey({
    role: "agent",
    account_id: accounts.agent,
    public_key: fcKey.public_key,
    secret_key: fcKey.secret_key,
  });

  const walletAccount = await viewAccount(RPC, accounts.wallet);
  const agentView = await viewCall(RPC, accounts.wallet, "get_agent", { agent_id: accounts.agent });
  if (!agentView || agentView.active !== true) fail("get_agent did not show an active agent");
  if (!walletAccount || !walletAccount.code_hash) fail("wallet code hash was not readable");

  const injected = idFor("injected");
  const previous = JSON.parse(readFileSync(join(ROOT, "config.json"), "utf8"));
  const config = {
    ...previous,
    ready: true,
    network: "testnet",
    rpcUrl: RPC,
    accounts: {
      funder: accounts.funder,
      owner: accounts.owner,
      agent: accounts.agent,
      wallet: accounts.wallet,
      token: TOKEN,
      payee: accounts.payee,
      injected,
      flagged: accounts.flagged,
    },
    creationTx: state.creationTx,
    policy: {
      perCallMax: PER_CALL.toString(),
      dailyMax: DAILY.toString(),
      allowedAmount: ALLOWED.toString(),
      decimals: 24,
      payees: [accounts.payee],
    },
    flaggedAccounts: [accounts.flagged],
    verifiedCodeHashes: [walletAccount.code_hash],
  };
  assertPublicConfig(config);
  writeFileSync(join(ROOT, "config.json"), JSON.stringify(config, null, 2) + "\n");
  console.log("public config written to demo/live/config.json");
  console.log(`owner ${accounts.owner}`);
  console.log(`agent ${accounts.agent}`);
  console.log(`wallet ${accounts.wallet}`);
  console.log(`payee ${accounts.payee}`);
  console.log(`flagged ${accounts.flagged}`);
  console.log(`injected ${injected} (not created)`);
  console.log("signing keys are in demo/live/.keys (gitignored)");
  console.log("Vercel: TRIAGEPAY_AGENT_SECRET_KEY from .keys/agent.json");
  console.log("Vercel: TRIAGEPAY_OWNER_SECRET_KEY from .keys/owner.json");
}

async function cleanup() {
  await assertTestnet(RPC);
  const config = JSON.parse(readFileSync(join(ROOT, "config.json"), "utf8"));
  const agentId = config.accounts && config.accounts.agent;
  const walletId = config.accounts && config.accounts.wallet;
  if (!agentId || !walletId) fail("config.json has no agent or wallet account");
  const envKey = String(process.env.TRIAGEPAY_AGENT_FULL_SECRET_KEY || "").trim();
  let full = null;
  if (envKey) full = { account_id: agentId, secret_key: envKey };
  else if (existsSync(keyPath("agent-full"))) full = loadKey("agent-full");
  else fail("cleanup needs TRIAGEPAY_AGENT_FULL_SECRET_KEY or demo/live/.keys/agent-full.json", 2);
  const keys = await viewAccessKeys(RPC, agentId);
  let keepPublicKey = null;
  if (existsSync(keyPath("agent"))) {
    try {
      keepPublicKey = loadKey("agent").public_key;
    } catch {
      keepPublicKey = null;
    }
  }
  const plan = selectKeysToDelete(keys, {
    walletId,
    keepPublicKey,
    dropFullAccess: process.argv.includes("--drop-full-access"),
  });
  console.log(`agent ${agentId}`);
  console.log(`keep ${plan.keep || "(no pay key for this wallet)"}`);
  console.log(`delete ${plan.drop.length ? plan.drop.join(" ") : "(none)"}`);
  if (!plan.drop.length) return;
  const result = await signCall({
    rpcUrl: RPC,
    signerId: full.account_id || agentId,
    secretKey: full.secret_key,
    receiverId: agentId,
    actions: plan.drop.map((publicKey) => deleteKey(publicKey)),
  });
  if (result.rejected || result.outcome !== "success") {
    fail(`cleanup failed (${result.errorCode || result.detail || result.outcome})`);
  }
  console.log(`cleanup https://testnet.nearblocks.io/txns/${result.txHash}`);
  if (!process.argv.includes("--drop-full-access")) {
    console.log("The full-access key is still on the account. Pass --drop-full-access to remove it after a pay key is the one you will keep. An account with only a function-call key can lock itself out when the allowance runs out.");
  }
}

const command = process.argv[2];
if (command === "cleanup") {
  cleanup().catch((error) => fail(error && error.stack ? error.stack : error));
} else {
  main().catch((error) => fail(error && error.stack ? error.stack : error));
}
