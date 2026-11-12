// Live testnet proof for the owner-review wallet that setup.mjs deployed.
//
//   node proof.mjs          run the payment proof and write the evidence
//   node proof.mjs adopt    add the hosted demo's existing public keys
//   node proof.mjs finish   drop every agent key except the hosted pay key
//
// Keys come from demo/live/.keys (gitignored). Nothing secret is printed or
// written to the evidence file: it holds account ids, public keys, amounts
// and transaction hashes only.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { redact } from "./lib/http.js";
import {
  addPayKey,
  assertTestnet,
  deleteKey,
  functionCall,
  near,
  parseNear,
  signCall,
  viewAccessKeys,
  viewCall,
} from "./lib/near.js";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const KEYS = join(ROOT, ".keys");
const OUT = join(REPO, "docs", "demo", "testnet-review", "2026-11-12");
const EVIDENCE = join(OUT, "EVIDENCE.json");
const config = JSON.parse(readFileSync(join(ROOT, "config.json"), "utf8"));
const RPC = config.rpcUrl;
const A = config.accounts;
const ALLOWED = BigInt(config.policy.allowedAmount);
const PER_CALL = BigInt(config.policy.perCallMax);
const TX = "https://testnet.nearblocks.io/txns/";
const ONE_YOCTO = 1n;

const key = (role) => JSON.parse(readFileSync(join(KEYS, role + ".json"), "utf8"));
const evidence = existsSync(EVIDENCE)
  ? JSON.parse(readFileSync(EVIDENCE, "utf8"))
  : { network: "testnet", accounts: { ...A }, steps: [] };

function save() {
  mkdirSync(OUT, { recursive: true });
  const text = JSON.stringify(evidence, null, 2) + "\n";
  if (/ed25519:[1-9A-HJ-NP-Za-km-z]{60,}/.test(text) || /secret_key/.test(text)) {
    throw new Error("evidence would contain a secret");
  }
  writeFileSync(EVIDENCE, text);
}

async function balance(account) {
  return BigInt(await viewCall(RPC, A.token, "ft_balance_of", { account_id: account }));
}

async function send(name, role, receiverId, actions, expect) {
  const k = key(role);
  const result = await signCall({ rpcUrl: RPC, signerId: k.account_id, secretKey: k.secret_key, receiverId, actions });
  const ok = expect === "success" ? result.outcome === "success" : result.errorCode === expect;
  const entry = {
    name,
    signer: k.account_id,
    signerPublicKey: k.public_key,
    receiver: receiverId,
    txHash: result.txHash,
    url: result.txHash ? TX + result.txHash : null,
    outcome: result.outcome,
    errorCode: result.errorCode,
    expected: expect,
    ok,
  };
  console.log(`${ok ? "OK  " : "FAIL"} ${name} ${result.outcome}${result.errorCode ? " " + result.errorCode : ""} ${entry.url || redact(result.detail || "")}`);
  evidence.steps.push(entry);
  save();
  if (!ok) throw new Error(`${name} did not match ${expect}`);
  return entry;
}

const pay = (receiver, amount, memo) =>
  functionCall("pay", { token: A.token, receiver_id: receiver, amount: amount.toString(), memo }, 50, 0n);

async function pendingFor(amount) {
  const rows = await viewCall(RPC, A.wallet, "get_pending", {});
  const row = (rows || []).filter((r) => r.amount === amount.toString()).pop();
  if (!row) throw new Error("no pending row for " + amount);
  return row;
}

async function proof() {
  await assertTestnet(RPC);
  evidence.review = await viewCall(RPC, A.wallet, "get_review_policy", {});
  evidence.policy = { perCallMax: PER_CALL.toString(), allowedAmount: ALLOWED.toString() };
  const start = { payee: await balance(A.payee), wallet: await balance(A.wallet) };
  evidence.balances = { start: { payee: start.payee.toString(), wallet: start.wallet.toString() } };

  await send("allowed_payment", "agent", A.wallet, [await pay(A.payee, ALLOWED, "proof:allowed")], "success");
  const afterAllowed = await balance(A.payee);
  evidence.balances.afterAllowed = afterAllowed.toString();
  if (afterAllowed - start.payee !== ALLOWED) throw new Error("allowed payment did not reach the payee");

  await send("injected_payee_refused", "agent", A.wallet, [await pay(A.injected, ALLOWED, "proof:injected")], "ERR_PAYEE_NOT_ALLOWED");
  await send("over_limit_refused", "agent", A.wallet, [await pay(A.payee, PER_CALL + 1n, "proof:over-limit")], "ERR_PER_CALL_LIMIT");

  const held = ALLOWED * 2n;
  await send("above_threshold_queued", "agent", A.wallet, [await pay(A.payee, held, "proof:review-approve")], "success");
  const afterQueue = await balance(A.payee);
  if (afterQueue !== afterAllowed) throw new Error("queued payment moved funds before approval");
  await approveAndReject(afterAllowed, held);
}

// Split out so a run that stopped after the queue step can continue.
async function approveAndReject(afterAllowed, held) {
  const row = await pendingFor(held);
  evidence.queuedForApproval = row;
  await send("owner_approved", "owner", A.wallet, [await functionCall("approve_payment", { payment_id: String(row.payment_id) }, 80, ONE_YOCTO)], "success");
  const afterApprove = await balance(A.payee);
  evidence.balances.afterApprove = afterApprove.toString();
  if (afterApprove - afterAllowed !== held) throw new Error("approved payment did not settle");

  const rejected = ALLOWED * 3n;
  await send("second_queued", "agent", A.wallet, [await pay(A.payee, rejected, "proof:review-reject")], "success");
  const row2 = await pendingFor(rejected);
  evidence.queuedForRejection = row2;
  await send("owner_rejected", "owner", A.wallet, [await functionCall("reject_payment", { payment_id: String(row2.payment_id) }, 30, ONE_YOCTO)], "success");
  const afterReject = await balance(A.payee);
  if (afterReject !== afterApprove) throw new Error("rejected payment moved funds");
  evidence.pendingAfterReject = await viewCall(RPC, A.wallet, "get_pending", {});

  await send("owner_revoked", "owner", A.wallet, [await functionCall("revoke_agent", { agent_id: A.agent }, 30, ONE_YOCTO)], "success");
  await send("revoked_agent_refused", "agent", A.wallet, [await pay(A.payee, ALLOWED, "proof:revoked")], "ERR_AGENT_INACTIVE");
  await send("owner_restored", "owner", A.wallet, [await functionCall("set_agent", {
    agent_id: A.agent,
    per_call_max: config.policy.perCallMax,
    daily_max: config.policy.dailyMax,
    payees: config.policy.payees,
  }, 30, ONE_YOCTO)], "success");

  evidence.balances.end = { payee: (await balance(A.payee)).toString(), wallet: (await balance(A.wallet)).toString() };
  evidence.spentToday = await viewCall(RPC, A.wallet, "get_spent", { agent_id: A.agent });
  evidence.agentAfter = await viewCall(RPC, A.wallet, "get_agent", { agent_id: A.agent });
  save();
}

// The hosted Vercel project already holds a pay key and an owner key for
// the previous wallet. Adding their public halves here lets the same
// secrets sign for the new accounts. Only public keys are read.
async function adopt() {
  const agentPub = process.env.HOSTED_AGENT_PUBLIC_KEY;
  const ownerPub = process.env.HOSTED_OWNER_PUBLIC_KEY;
  if (!agentPub || !ownerPub) throw new Error("set HOSTED_AGENT_PUBLIC_KEY and HOSTED_OWNER_PUBLIC_KEY");
  const did = (name) => evidence.steps.some((s) => s.name === name && s.ok);
  if (!did("hosted_agent_pay_key")) await send("hosted_agent_pay_key", "agent-full", A.agent, [addPayKey(agentPub, A.wallet, parseNear("0.25"))], "success");
  await send("hosted_owner_key", "owner", A.owner, [near().actions.addFullAccessKey(near().PublicKey.fromString(ownerPub))], "success");
  evidence.hostedKeys = { agentPayKey: agentPub, ownerKey: ownerPub };
  save();
}

// Leave the agent with one key: the hosted pay key. The local pay key and
// the setup full-access key are deleted in one transaction signed by the
// full-access key.
async function finish() {
  const keep = evidence.hostedKeys && evidence.hostedKeys.agentPayKey;
  if (!keep) throw new Error("run adopt first");
  const keys = await viewAccessKeys(RPC, A.agent);
  const list = Array.isArray(keys) ? keys : [];
  const drop = list.map((k) => k.public_key).filter((pk) => pk !== keep);
  if (!list.some((k) => k.public_key === keep)) throw new Error("hosted pay key is not on the agent");
  await send("agent_keys_cleanup", "agent-full", A.agent, drop.map((pk) => deleteKey(pk)), "success");
  const after = await viewAccessKeys(RPC, A.agent);
  evidence.agentKeysAfter = (Array.isArray(after) ? after : []).map((k) => ({ public_key: k.public_key, permission: k.access_key.permission }));
  save();
  console.log(JSON.stringify(evidence.agentKeysAfter));
}

const cmd = process.argv[2] || "proof";
async function resume() {
  // The first run sent approve_payment with an undefined id; the contract
  // refused to parse it. Keep that transaction in the record and continue.
  const bad = evidence.steps.filter((s) => !s.ok);
  evidence.failedAttempts = [...(evidence.failedAttempts || []), ...bad.map((s) => ({ ...s, note: "script bug: payment_id was undefined; the wallet refused to parse the call and changed nothing" }))];
  evidence.steps = evidence.steps.filter((s) => s.ok);
  save();
  await approveAndReject(await balance(A.payee), ALLOWED * 2n);
}

const run = cmd === "adopt" ? adopt : cmd === "finish" ? finish : cmd === "resume" ? resume : proof;
run().catch((error) => {
  console.error("STOP: " + redact(error && error.message ? error.message : String(error)));
  process.exit(1);
});
