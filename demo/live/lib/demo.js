// One signed attempt, the read-only dashboard, and the owner kill switch.
// Attack scenarios are submitted on purpose: the page's point is the
// wallet's on-chain refusal, with the risk verdict beside it.

import { describeAttack, explain, listScenarios, shouldSubmit } from "./attacks.js";
import { chainReady, loadConfig, loadKeys, signingReady } from "./config.js";
import { assessCooldown, cooldownMessage } from "./cooldown.js";
import { payeeFacts, fetchIndexerTxns } from "./facts.js";
import { formatAtomic, nsToIso } from "./format.js";
import { DemoError } from "./http.js";
import { recall, remember } from "./journal.js";
import { functionCall, signCall, viewCall, assertTestnet } from "./near.js";
import { classifyAmount, classifyPayee } from "./risk.js";
import { mergeRecent } from "./recent.js";

let signing = false;
const ONE_YOCTO = 1n;

function requireChain(config) {
  if (!chainReady(config)) {
    throw new DemoError(
      "demo_not_configured",
      "The demo wallet is not configured. Run node setup.mjs and commit the public account ids, then set the signing keys.",
      503,
    );
  }
}

function requireKeys(keys) {
  if (!signingReady(keys)) {
    throw new DemoError(
      "keys_not_configured",
      "Set TRIAGEPAY_AGENT_SECRET_KEY and TRIAGEPAY_OWNER_SECRET_KEY on the server. They are not in the page.",
      503,
    );
  }
}

function asBig(value) {
  if (value == null) return 0n;
  return BigInt(value);
}

async function gate(config) {
  const indexed = await fetchIndexerTxns(config.indexerAccountsBase, config.accounts.agent);
  const stamps = [
    ...indexed.map((row) => row.atMs).filter((value) => value != null),
    ...recall().map((row) => row.atMs),
  ];
  const decision = assessCooldown({
    nowMs: Date.now(),
    timestampsMs: stamps,
    cooldownMs: config.cooldownSeconds * 1000,
    maxPerDay: config.maxAttemptsPerDay,
  });
  if (!decision.allowed) {
    throw new DemoError(
      decision.code,
      cooldownMessage(decision, config.cooldownSeconds, config.maxAttemptsPerDay),
      429,
      { retry_after_seconds: decision.retryAfterSeconds, attempts_today: decision.attemptsToday },
    );
  }
  return decision;
}

function riskInput(config, facts) {
  return {
    ...facts,
    allowlist: config.policy.payees || [],
    flagged: config.flaggedAccounts || [],
    verifiedCodeHashes: config.verifiedCodeHashes || [],
    newAccountWindowMs: config.newAccountWindowMs,
  };
}

async function classifyAccount(config, accountId) {
  const creation = (config.creationTx && config.creationTx[accountId]) || null;
  const facts = await payeeFacts({
    rpcUrl: config.rpcUrl,
    accountId,
    creation,
    nowMs: Date.now(),
    indexerBase: config.indexerAccountsBase,
  });
  const risk = classifyPayee(riskInput(config, facts));
  return {
    accountId,
    exists: facts.exists,
    codeHash: facts.codeHash,
    ageSource: facts.ageSource,
    createdAt: facts.createdAtMs ? new Date(facts.createdAtMs).toISOString() : null,
    ...risk,
  };
}

function scenarioView(scenario) {
  return {
    id: scenario.id,
    kind: scenario.kind,
    title: scenario.title,
    injection: scenario.injection,
    payee: scenario.payee,
    amount: scenario.amount,
    amountDisplay: scenario.amountDisplay,
    expectedError: scenario.expectedError,
    narrative: scenario.narrative,
  };
}

export async function getHealth() {
  const config = loadConfig();
  let keys = { agent: null, owner: null };
  let keyError = null;
  try {
    keys = loadKeys();
  } catch (error) {
    keyError = error instanceof DemoError ? error.message : "signing key was rejected";
  }
  return {
    ok: true,
    network: config.network,
    configured: chainReady(config),
    signing: signingReady(keys),
    keyError,
    wallet: config.accounts.wallet || null,
    agent: config.accounts.agent || null,
    cooldownSeconds: config.cooldownSeconds,
    maxAttemptsPerDay: config.maxAttemptsPerDay,
  };
}

export async function getDashboard() {
  const config = loadConfig();
  const health = await getHealth();
  if (!chainReady(config)) {
    return {
      ...health,
      message: "The public account ids are not in config.json yet.",
      scenarios: [],
      risks: [],
      recent: [],
      agent: null,
    };
  }
  await assertTestnet(config.rpcUrl);
  const decimals = config.policy.decimals;
  const [walletConfig, agent, spent, indexed] = await Promise.all([
    viewCall(config.rpcUrl, config.accounts.wallet, "get_config", {}),
    viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", { agent_id: config.accounts.agent }),
    viewCall(config.rpcUrl, config.accounts.wallet, "get_spent", { agent_id: config.accounts.agent }),
    fetchIndexerTxns(config.indexerAccountsBase, config.accounts.agent),
  ]);
  const previewIds = [
    config.accounts.payee,
    config.accounts.injected,
    config.accounts.flagged,
    config.accounts.token,
  ].filter(Boolean);
  const risks = await Promise.all(previewIds.map((id) => classifyAccount(config, id)));
  const spentToday = agent ? asBig(agent.spent_in_window) : asBig(spent);
  const dailyMax = agent ? asBig(agent.daily_max) : asBig(config.policy.dailyMax);
  const perCall = agent ? asBig(agent.per_call_max) : asBig(config.policy.perCallMax);
  const remaining = agent ? asBig(agent.remaining) : dailyMax - spentToday;
  let percent = 0;
  if (dailyMax > 0n) {
    const scaled = (spentToday * 10000n) / dailyMax;
    percent = Math.max(0, Math.min(100, Number(scaled) / 100));
  }
  return {
    ...health,
    wallet: config.accounts.wallet,
    token: config.accounts.token,
    payee: config.accounts.payee,
    owner: config.accounts.owner,
    explorerAccount: config.explorerAccountBase,
    paused: Boolean(walletConfig && walletConfig.paused),
    spend: {
      spentToday: spentToday.toString(),
      spentDisplay: formatAtomic(spentToday, decimals),
      dailyMax: dailyMax.toString(),
      dailyDisplay: formatAtomic(dailyMax, decimals),
      remaining: remaining.toString(),
      remainingDisplay: formatAtomic(remaining < 0n ? 0n : remaining, decimals),
      perCallMax: perCall.toString(),
      perCallDisplay: formatAtomic(perCall, decimals),
      percent,
      active: Boolean(agent && agent.active),
      payees: (agent && agent.payees) || config.policy.payees,
      resetsAt: agent ? nsToIso(agent.resets_at_ns) : null,
    },
    scenarios: listScenarios(config).map(scenarioView),
    risks,
    recent: mergeRecent(indexed, recall(), config.explorerTxBase),
  };
}

async function submitPay(config, keys, attack) {
  const action = await functionCall(
    "pay",
    {
      token: config.accounts.token,
      receiver_id: attack.payee,
      amount: attack.amount,
      memo: `demo:${attack.id}`.slice(0, 256),
    },
    50,
    0n,
  );
  return signCall({
    rpcUrl: config.rpcUrl,
    signerId: config.accounts.agent,
    secretKey: keys.agent,
    receiverId: config.accounts.wallet,
    actions: [action],
  });
}

export async function runScenario(id) {
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  const attack = describeAttack(id, config);
  if (!attack) throw new DemoError("unknown_attack", "unknown scenario", 400);
  await assertTestnet(config.rpcUrl);
  if (signing) {
    throw new DemoError("cooldown", "A signature is already in progress.", 429, { retry_after_seconds: 2 });
  }
  signing = true;
  try {
    const decision = await gate(config);
    const agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
      agent_id: config.accounts.agent,
    });
    const facts = await payeeFacts({
      rpcUrl: config.rpcUrl,
      accountId: attack.payee,
      creation: (config.creationTx && config.creationTx[attack.payee]) || null,
      nowMs: Date.now(),
      indexerBase: config.indexerAccountsBase,
    });
    const risk = classifyPayee(riskInput(config, facts));
    const amountCheck = classifyAmount({
      amount: attack.amount,
      perCallMax: agent ? agent.per_call_max : config.policy.perCallMax,
      spentToday: agent ? agent.spent_in_window : "0",
      dailyMax: agent ? agent.daily_max : config.policy.dailyMax,
    });
    const chain = { submitted: false, txHash: null, explorerUrl: null, outcome: null, errorCode: null, rejected: false };
    if (shouldSubmit(attack, risk, amountCheck)) {
      const signed = await submitPay(config, keys, attack);
      chain.submitted = true;
      chain.txHash = signed.txHash;
      chain.explorerUrl = signed.txHash ? config.explorerTxBase + signed.txHash : null;
      chain.outcome = signed.outcome;
      chain.errorCode = signed.errorCode;
      chain.rejected = Boolean(signed.rejected);
      chain.detail = signed.detail || null;
      remember({
        atMs: Date.now(),
        kind: attack.kind,
        title: attack.title,
        payee: attack.payee,
        amount: attack.amount,
        riskVerdict: risk.verdict,
        outcome: signed.outcome,
        errorCode: signed.errorCode,
        txHash: signed.txHash,
      });
    }
    return {
      scenario: scenarioView(attack),
      risk: { ...risk, ageSource: facts.ageSource, exists: facts.exists, codeHash: facts.codeHash },
      amountCheck,
      chain,
      summary: explain(attack, risk, amountCheck, chain),
      attemptsToday: decision.attemptsToday + (chain.submitted ? 1 : 0),
    };
  } finally {
    signing = false;
  }
}

export async function setSpending(action) {
  if (action !== "revoke" && action !== "restore") {
    throw new DemoError("bad_action", "action must be revoke or restore", 400);
  }
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  await assertTestnet(config.rpcUrl);
  if (signing) {
    throw new DemoError("cooldown", "A signature is already in progress.", 429, { retry_after_seconds: 2 });
  }
  signing = true;
  try {
    await gate(config);
    const agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
      agent_id: config.accounts.agent,
    });
    const active = Boolean(agent && agent.active);
    if (action === "revoke" && !active) {
      return { action, changed: false, summary: "Spending is already revoked. The agent key cannot pay until the owner restores it." };
    }
    if (action === "restore" && active) {
      return { action, changed: false, summary: "Spending is already enabled." };
    }
    const call = action === "revoke"
      ? await functionCall("revoke_agent", { agent_id: config.accounts.agent }, 30, ONE_YOCTO)
      : await functionCall(
        "set_agent",
        {
          agent_id: config.accounts.agent,
          per_call_max: config.policy.perCallMax,
          daily_max: config.policy.dailyMax,
          payees: config.policy.payees,
        },
        30,
        ONE_YOCTO,
      );
    const signed = await signCall({
      rpcUrl: config.rpcUrl,
      signerId: config.accounts.owner,
      secretKey: keys.owner,
      receiverId: config.accounts.wallet,
      actions: [call],
    });
    remember({
      atMs: Date.now(),
      kind: action,
      title: action === "revoke" ? "Revoke spending" : "Restore spending",
      outcome: signed.outcome,
      errorCode: signed.errorCode,
      txHash: signed.txHash,
    });
    const summary = signed.outcome === "success"
      ? action === "revoke"
        ? "The owner revoked the agent. The key still exists, and the next pay fails closed."
        : "The owner restored the agent. The day's spend was kept."
      : `The owner transaction did not succeed${signed.errorCode ? ` (${signed.errorCode})` : ""}.`;
    return {
      action,
      changed: signed.outcome === "success",
      chain: {
        submitted: true,
        txHash: signed.txHash,
        explorerUrl: signed.txHash ? config.explorerTxBase + signed.txHash : null,
        outcome: signed.outcome,
        errorCode: signed.errorCode,
        rejected: Boolean(signed.rejected),
      },
      summary,
    };
  } finally {
    signing = false;
  }
}
