// Read-only check of the live policy wallets. No key, no signature, no write.
// Expected values live in expected-live.json so another mainnet wallet is a
// new entry, not a code change. A wallet's spend and payment counter move,
// so those views are not pinned.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const PAY_METHOD = "pay";

export function checkKeys(mode, walletId, keys) {
  const list = keys || [];
  if (mode == null) return [];
  if (mode === "none") {
    return list.length === 0 ? [] : ["agent account still has access keys"];
  }
  if (mode !== "pay-only") return [`unknown key mode ${mode}`];
  if (list.length === 0) return ["agent account has no pay key"];
  const problems = [];
  for (const key of list) {
    const permission = key && key.permission;
    if (!permission || permission === "FullAccess" || permission.FullAccess != null) {
      problems.push("agent account has a full-access key");
      continue;
    }
    const call = permission.FunctionCall;
    if (!call) {
      problems.push("agent key is not a function-call key");
      continue;
    }
    if (call.receiver_id !== walletId) {
      problems.push(`function-call key receiver is ${call.receiver_id}, not ${walletId}`);
    }
    const methods = call.method_names || [];
    if (methods.length !== 1 || methods[0] !== PAY_METHOD) {
      problems.push(`function-call key methods are ${JSON.stringify(methods)}, not ["pay"]`);
    }
  }
  return problems;
}

export function checkWallet(spec, observed) {
  const problems = [];
  if (!observed || !observed.exists) {
    problems.push(`${spec.accountId} does not exist`);
    return problems;
  }
  if (observed.codeHash !== spec.codeHash) {
    problems.push(`code hash is ${observed.codeHash}, expected ${spec.codeHash}`);
  }
  const config = observed.config || {};
  if (config.owner !== spec.config.owner) {
    problems.push(`owner is ${config.owner}, expected ${spec.config.owner}`);
  }
  if (config.paused !== spec.config.paused) {
    problems.push(`paused is ${config.paused}, expected ${spec.config.paused}`);
  }
  if (JSON.stringify(config.tokens || []) !== JSON.stringify(spec.config.tokens)) {
    problems.push(`tokens are ${JSON.stringify(config.tokens)}, expected ${JSON.stringify(spec.config.tokens)}`);
  }
  const agent = observed.agent;
  if (!agent) {
    problems.push(`agent ${spec.agent.accountId} has no policy`);
  } else {
    if (String(agent.per_call_max) !== spec.agent.perCallMax) {
      problems.push(`per-call cap is ${agent.per_call_max}, expected ${spec.agent.perCallMax}`);
    }
    if (String(agent.daily_max) !== spec.agent.dailyMax) {
      problems.push(`daily cap is ${agent.daily_max}, expected ${spec.agent.dailyMax}`);
    }
    if (JSON.stringify(agent.payees || []) !== JSON.stringify(spec.agent.payees)) {
      problems.push(`allowlist is ${JSON.stringify(agent.payees)}, expected ${JSON.stringify(spec.agent.payees)}`);
    }
    if (agent.active !== spec.agent.active) {
      problems.push(`agent active is ${agent.active}, expected ${spec.agent.active}`);
    }
  }
  if (spec.review) {
    const review = observed.review || {};
    if (String(review.approval_threshold) !== spec.review.approvalThreshold) {
      problems.push(`approval threshold is ${review.approval_threshold}, expected ${spec.review.approvalThreshold}`);
    }
    if (review.review_unknown_payees !== spec.review.reviewUnknownPayees) {
      problems.push(`review_unknown_payees is ${review.review_unknown_payees}, expected ${spec.review.reviewUnknownPayees}`);
    }
    if (spec.review.maxPending != null && Number(review.max_pending) !== spec.review.maxPending) {
      problems.push(`queue capacity is ${review.max_pending}, expected ${spec.review.maxPending}`);
    }
  }
  problems.push(...checkKeys(spec.agent.keys, spec.accountId, observed.keys));
  if (spec.agent.publicKey) {
    const present = (observed.keys || []).some((key) => key.public_key === spec.agent.publicKey);
    if (!present) problems.push(`pay key ${spec.agent.publicKey} is not on the agent account`);
  }
  if (spec.walletKeys === "none" && (observed.walletKeys || []).length !== 0) {
    problems.push("wallet account has access keys");
  }
  return problems;
}

export function checkProof(spec, observed) {
  const problems = [];
  if (!observed) {
    return [`${spec.label} transaction was not returned`];
  }
  if (observed.finalExecutionStatus !== "FINAL") {
    problems.push(`${spec.label} finality is ${observed.finalExecutionStatus}, expected FINAL`);
  }
  if (spec.outcome === "success") {
    if (!observed.success) problems.push(`${spec.label} did not succeed`);
    for (const name of spec.events || []) {
      if (!(observed.events || []).includes(name)) {
        problems.push(`${spec.label} is missing event ${name}`);
      }
    }
  } else if (observed.success || !String(observed.failureText || "").includes(spec.errorCode)) {
    problems.push(`${spec.label} did not fail with ${spec.errorCode}`);
  }
  return problems;
}

export function checkFactory(spec, observed) {
  if (!spec || spec.live !== true) return [];
  const problems = [];
  if (!observed || !observed.exists) {
    problems.push(`${spec.accountId} does not exist`);
    return problems;
  }
  if (spec.codeHash && observed.codeHash !== spec.codeHash) {
    problems.push(`factory code hash is ${observed.codeHash}, expected ${spec.codeHash}`);
  }
  const config = observed.config || {};
  if (spec.globalCodeHash && config.code_hash_hex !== spec.globalCodeHash) {
    problems.push(`global contract hash is ${config.code_hash_hex}, expected ${spec.globalCodeHash}`);
  }
  if (spec.feeRecipient && config.fee_recipient !== spec.feeRecipient) {
    problems.push(`fee recipient is ${config.fee_recipient}, expected ${spec.feeRecipient}`);
  }
  return problems;
}

export function checkHealth(spec, observed) {
  const problems = [];
  if (!observed || observed.status !== 200) {
    problems.push(`health HTTP status is ${observed && observed.status}`);
    return problems;
  }
  const body = observed.body || {};
  if (body.ok !== spec.ok) problems.push(`health ok is ${body.ok}, expected ${spec.ok}`);
  if (spec.wallet && body.wallet !== spec.wallet) {
    problems.push(`health wallet is ${body.wallet}, expected ${spec.wallet}`);
  }
  return problems;
}

function rpcUrl(spec, network) {
  return network === "testnet" ? spec.testnetRpc : spec.mainnetRpc;
}

const RETRYABLE = new Set([408, 429, 500, 502, 503, 504]);

export function archivalUrl(url) {
  return /testnet/.test(url)
    ? "https://archival-rpc.testnet.fastnear.com"
    : "https://archival-rpc.mainnet.fastnear.com";
}

async function rpc(url, method, params, attempts = 3) {
  let response;
  for (let attempt = 1; ; attempt++) {
    response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "triagepay-live-check", method, params }),
    });
    // A busy public node answers 408/429/5xx now and then; that says
    // nothing about the wallet, so retry before reporting a problem.
    if (response.ok || !RETRYABLE.has(response.status) || attempt >= attempts) break;
    await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
  }
  if (!response.ok) throw new Error(`${method} HTTP ${response.status}`);
  const body = await response.json();
  if (body.error) throw new Error(`${method}: ${body.error.message || JSON.stringify(body.error)}`);
  return body.result;
}

function decodeCall(result) {
  const raw = Buffer.from(result.result).toString("utf8");
  return raw ? JSON.parse(raw) : null;
}

async function viewCall(url, accountId, method, args) {
  const result = await rpc(url, "query", {
    request_type: "call_function",
    finality: "final",
    account_id: accountId,
    method_name: method,
    args_base64: Buffer.from(JSON.stringify(args)).toString("base64"),
  });
  return decodeCall(result);
}

export async function observeWallet(url, spec) {
  let account;
  try {
    account = await rpc(url, "query", {
      request_type: "view_account",
      finality: "final",
      account_id: spec.accountId,
    });
  } catch (error) {
    if (String(error.message).includes("does not exist")) return { exists: false };
    throw error;
  }
  const config = await viewCall(url, spec.accountId, "get_config", {});
  const agent = await viewCall(url, spec.accountId, "get_agent", { agent_id: spec.agent.accountId });
  const review = spec.review
    ? await viewCall(url, spec.accountId, "get_review_policy", {})
    : null;
  const normalize = (listed) => (listed.keys || []).map((key) => ({
    public_key: key.public_key,
    permission: key.access_key ? key.access_key.permission : key.permission,
  }));
  let keys = [];
  if (spec.agent.keys != null || spec.agent.publicKey) {
    const listed = await rpc(url, "query", {
      request_type: "view_access_key_list",
      finality: "final",
      account_id: spec.agent.accountId,
    });
    keys = normalize(listed);
  }
  let walletKeys = [];
  if (spec.walletKeys != null) {
    const listed = await rpc(url, "query", {
      request_type: "view_access_key_list",
      finality: "final",
      account_id: spec.accountId,
    });
    walletKeys = normalize(listed);
  }
  return { exists: true, codeHash: account.code_hash, config, agent, review, keys, walletKeys };
}

function eventNames(result) {
  const names = [];
  for (const outcome of result.receipts_outcome || []) {
    for (const line of outcome.outcome?.logs || []) {
      if (!line.startsWith("EVENT_JSON:")) continue;
      try {
        names.push(JSON.parse(line.slice("EVENT_JSON:".length)).event);
      } catch {
        // A non-event log is not one of the proof events.
      }
    }
  }
  return names;
}

export async function observeProof(url, spec) {
  const params = { tx_hash: spec.hash, sender_account_id: spec.signerId, wait_until: "NONE" };
  let result;
  try {
    result = await rpc(url, "EXPERIMENTAL_tx_status", params);
  } catch {
    // Regular nodes drop transactions after about five epochs; the
    // archival node keeps them.
    result = await rpc(archivalUrl(url), "EXPERIMENTAL_tx_status", params);
  }
  const status = result.status || {};
  const success = Object.prototype.hasOwnProperty.call(status, "SuccessValue")
    || Object.prototype.hasOwnProperty.call(status, "SuccessReceiptId");
  return {
    finalExecutionStatus: result.final_execution_status,
    success,
    failureText: success ? "" : JSON.stringify(status),
    events: eventNames(result),
  };
}

export async function observeFactory(url, spec) {
  let account;
  try {
    account = await rpc(url, "query", {
      request_type: "view_account",
      finality: "final",
      account_id: spec.accountId,
    });
  } catch (error) {
    if (String(error.message).includes("does not exist")) return { exists: false };
    throw error;
  }
  const config = await viewCall(url, spec.accountId, "get_config", {});
  return { exists: true, codeHash: account.code_hash, config };
}

export async function observeHealth(url) {
  const response = await fetch(url);
  const text = await response.text();
  let body = null;
  try {
    body = JSON.parse(text);
  } catch {
    body = null;
  }
  return { status: response.status, body };
}

function printProblems(title, problems) {
  if (problems.length === 0) {
    console.log(`ok  ${title}`);
    return 0;
  }
  console.log(`FAIL ${title}`);
  for (const problem of problems) console.log(`    ${problem}`);
  return 1;
}

export async function run(spec, io = { log: console.log }) {
  let failed = 0;
  for (const wallet of spec.wallets) {
    const observed = await observeWallet(rpcUrl(spec, wallet.network), wallet);
    const problems = checkWallet(wallet, observed);
    failed += printProblems(`${wallet.network} ${wallet.accountId}`, problems);
  }
  for (const proof of spec.proofs) {
    const observed = await observeProof(rpcUrl(spec, proof.network), proof);
    failed += printProblems(`${proof.network} ${proof.label} ${proof.hash}`, checkProof(proof, observed));
  }
  const health = await observeHealth(spec.healthUrl);
  failed += printProblems(spec.healthUrl, checkHealth(spec.health, health));
  if (spec.factory && spec.factory.live === true) {
    const observed = await observeFactory(spec.mainnetRpc, spec.factory);
    failed += printProblems(`mainnet ${spec.factory.accountId}`, checkFactory(spec.factory, observed));
  }
  if (failed === 0) {
    io.log("live check passed");
  } else {
    io.log(`live check failed (${failed})`);
  }
  return failed;
}

const isDirect = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isDirect) {
  const spec = JSON.parse(readFileSync(new URL("./expected-live.json", import.meta.url), "utf8"));
  const failed = await run(spec);
  process.exit(failed === 0 ? 0 : 1);
}
