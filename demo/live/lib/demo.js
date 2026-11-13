// One signed attempt, the read-only dashboard, and the owner kill switch.
// Attack scenarios are submitted on purpose: the page's point is the
// wallet's on-chain refusal, with the risk verdict beside it.

import { describeAttack, explain, listScenarios, shouldSubmit } from "./attacks.js";
import { chainReady, loadConfig, loadKeys, signingReady } from "./config.js";
import { assessCooldown, cooldownMessage } from "./cooldown.js";
import { payeeFacts, fetchIndexerTxns, fetchDayStamps } from "./facts.js";
import { formatAmount, formatAtomic, nsToIso, tokenSymbol } from "./format.js";
import { DemoError } from "./http.js";
import { recall, remember } from "./journal.js";
import { clientIp, releaseLock, reserveAttempt } from "./limits.js";
import { functionCall, signCall, viewCall, assertTestnet } from "./near.js";
import { matchPending, receiptFromRun } from "./receipt.js";
import { classifyAmount, classifyPayee } from "./risk.js";
import { mergeRecent } from "./recent.js";
import { storeFromEnv } from "./store.js";
import { notifyHold } from "./telegram.js";
import { traceOutcome } from "./trace.js";
import { enrichRecent } from "./txread.js";
import { llmStatus, modelFromEnv } from "./agent.js";

let signing = false;
const ONE_YOCTO = 1n;

export function requireChain(config) {
  if (!chainReady(config)) {
    throw new DemoError(
      "demo_not_configured",
      "The demo wallet is not configured. Run node setup.mjs and commit the public account ids, then set the signing keys.",
      503,
    );
  }
}

export function requireKeys(keys) {
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

function limitMessage(decision, config) {
  if (decision.code === "ip_rate_limit") {
    return `This address has used its ${config.ipLimit} requests for this minute. Try again in ${decision.retryAfterSeconds} seconds.`;
  }
  if (decision.code === "limit_unavailable") {
    return "The attempt counter could not be read, so this server did not sign. Set the shared store or wait for the chain index.";
  }
  if (decision.code === "limit_store_unavailable") {
    return "The shared attempt store is configured and did not answer, so this server did not sign.";
  }
  return cooldownMessage(decision, config.cooldownSeconds, config.maxAttemptsPerDay);
}

async function chainDecision(config, nowMs) {
  const day = await fetchDayStamps(config.indexerAccountsBase, config.accounts.agent, nowMs);
  if (day.unavailable) return null;
  if (day.truncated) {
    const next = Date.UTC(new Date(nowMs).getUTCFullYear(), new Date(nowMs).getUTCMonth(), new Date(nowMs).getUTCDate()) + 86400000;
    return {
      allowed: false,
      code: "daily_attempt_cap",
      retryAfterSeconds: Math.max(1, Math.ceil((next - nowMs) / 1000)),
      attemptsToday: day.stamps.length,
    };
  }
  const newest = day.stamps.reduce((max, value) => (value > max ? value : max), 0);
  const extra = recall().map((row) => row.atMs).filter((value) => value && value > newest);
  return assessCooldown({
    nowMs,
    timestampsMs: [...day.stamps, ...extra],
    cooldownMs: config.cooldownSeconds * 1000,
    maxPerDay: config.maxAttemptsPerDay,
  });
}

export async function gate(config, request) {
  const nowMs = Date.now();
  const chain = await chainDecision(config, nowMs);
  const store = storeFromEnv();
  let decision;
  try {
    decision = await reserveAttempt({
      store,
      nowMs,
      ip: clientIp(request),
      maxPerDay: config.maxAttemptsPerDay,
      cooldownMs: config.cooldownSeconds * 1000,
      ipLimit: config.ipLimit || 4,
      ipWindowMs: (config.ipWindowSeconds || 60) * 1000,
      chain,
    });
  } catch {
    throw new DemoError(
      "limit_store_unavailable",
      "The shared attempt store is configured and did not answer, so this server did not sign.",
      503,
      { retry_after_seconds: 15 },
    );
  }
  if (!decision.allowed) {
    throw new DemoError(decision.code, limitMessage(decision, config), 429, {
      retry_after_seconds: decision.retryAfterSeconds,
      attempts_today: decision.attemptsToday,
      limit_source: decision.source,
    });
  }
  return { ...decision, store };
}

function riskInput(config, facts, chainPayees) {
  const fromChain = Array.isArray(chainPayees);
  return {
    ...facts,
    allowlist: fromChain ? chainPayees.map(String) : (config.policy.payees || []),
    allowlistSource: fromChain ? "chain" : "config",
    flagged: config.flaggedAccounts || [],
    verifiedCodeHashes: config.verifiedCodeHashes || [],
    newAccountWindowMs: config.newAccountWindowMs,
  };
}

async function classifyAccount(config, accountId, chainPayees) {
  const creation = (config.creationTx && config.creationTx[accountId]) || null;
  const facts = await payeeFacts({
    rpcUrl: config.rpcUrl,
    accountId,
    creation,
    nowMs: Date.now(),
    indexerBase: config.indexerAccountsBase,
  });
  const input = riskInput(config, facts, chainPayees);
  const risk = classifyPayee(input);
  return {
    accountId,
    exists: facts.exists,
    codeHash: facts.codeHash,
    ageSource: facts.ageSource,
    createdAt: facts.createdAtMs ? new Date(facts.createdAtMs).toISOString() : null,
    allowlistSource: input.allowlistSource,
    ...risk,
  };
}

export async function readReview(config) {
  try {
    const policy = await viewCall(config.rpcUrl, config.accounts.wallet, "get_review_policy", {});
    const pending = await viewCall(config.rpcUrl, config.accounts.wallet, "get_pending", {});
    return {
      supported: true,
      approvalThreshold: policy ? policy.approval_threshold : "0",
      reviewUnknownPayees: Boolean(policy && policy.review_unknown_payees),
      pendingCount: policy ? Number(policy.pending_count) : 0,
      maxPending: policy ? Number(policy.max_pending) : 0,
      pending: Array.isArray(pending) ? pending : [],
    };
  } catch (error) {
    if (error instanceof DemoError && error.code === "method_missing") {
      return {
        supported: false,
        reason: "This wallet build refuses unknown payees immediately. The approval queue is in the contract source and turns on after a new deploy.",
        pending: [],
      };
    }
    throw error;
  }
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
    ipLimit: config.ipLimit || 4,
    ipWindowSeconds: config.ipWindowSeconds || 60,
    sharedLimits: Boolean(storeFromEnv()),
    killSwitch: Boolean(String(process.env.KILL_SWITCH_SECRET || "").trim()),
    telegram: Boolean(String(process.env.TELEGRAM_BOT_TOKEN || "").trim() && String(process.env.TELEGRAM_OWNER_CHAT_ID || "").trim()),
    agentMode: String(process.env.NEARAI_API_KEY || "").trim() ? "live" : "scripted",
    policyApply: String(process.env.TRIAGEPAY_POLICY_APPLY || "") === "1",
    model: modelFromEnv(),
    llm: llmStatus(),
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
  const chainPayees = agent && Array.isArray(agent.payees) ? agent.payees : undefined;
  const [risks, review] = await Promise.all([
    Promise.all(previewIds.map((id) => classifyAccount(config, id, chainPayees))),
    readReview(config),
  ]);
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
      spentDisplay: formatAmount(spentToday, decimals, tokenSymbol(config.accounts.token)),
      dailyMax: dailyMax.toString(),
      dailyDisplay: formatAmount(dailyMax, decimals, tokenSymbol(config.accounts.token)),
      remaining: remaining.toString(),
      remainingDisplay: formatAmount(remaining < 0n ? 0n : remaining, decimals, tokenSymbol(config.accounts.token)),
      perCallMax: perCall.toString(),
      perCallDisplay: formatAmount(perCall, decimals, tokenSymbol(config.accounts.token)),
      percent,
      active: Boolean(agent && agent.active),
      payees: (agent && agent.payees) || config.policy.payees,
      allowlistSource: chainPayees ? "chain" : "config",
      resetsAt: agent ? nsToIso(agent.resets_at_ns) : null,
    },
    scenarios: listScenarios(config).map(scenarioView),
    risks,
    review: review && review.supported
      ? {
        ...review,
        approvalThresholdDisplay: formatAmount(
          review.approvalThreshold || "0",
          decimals,
          tokenSymbol(config.accounts.token),
        ),
        pending: (review.pending || []).map((row) => ({
          ...row,
          amountDisplay: row && row.amount != null
            ? formatAmount(String(row.amount), decimals, tokenSymbol(config.accounts.token))
            : null,
        })),
      }
      : review,
    cooldownRemainingSeconds: assessCooldown({
      nowMs: Date.now(),
      timestampsMs: (indexed || []).map((row) => row.atMs).filter((value) => Number.isFinite(value)),
      cooldownMs: config.cooldownSeconds * 1000,
      maxPerDay: config.maxAttemptsPerDay,
    }).retryAfterSeconds,
    recent: await enrichRecent(config, mergeRecent(indexed, recall(), config.explorerTxBase)),
  };
}

export async function signOwnerPolicy(request, { setAgent, setReview }) {
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  await assertTestnet(config.rpcUrl);
  const calls = [];
  if (setAgent) {
    calls.push(await functionCall("set_agent", {
      agent_id: config.accounts.agent,
      per_call_max: setAgent.perCallMax,
      daily_max: setAgent.dailyMax,
      payees: setAgent.payees,
    }, 40, ONE_YOCTO));
  }
  if (setReview) {
    calls.push(await functionCall("set_review_policy", {
      approval_threshold: setReview.approvalThreshold,
      review_unknown_payees: Boolean(setReview.reviewUnknownPayees),
    }, 20, ONE_YOCTO));
  }
  if (!calls.length) throw new DemoError("no_change", "Nothing in that sentence changes the on-chain policy.", 400);
  return withSigningLock(async ({ note }) => {
    const decision = await gate(config, request);
    note(decision);
    const signed = await signCall({
      rpcUrl: config.rpcUrl,
      signerId: config.accounts.owner,
      secretKey: keys.owner,
      receiverId: config.accounts.wallet,
      actions: calls,
    });
    const chain = chainView(config, signed);
    remember({
      atMs: Date.now(),
      kind: "policy",
      title: "Apply policy",
      outcome: signed.outcome,
      errorCode: signed.errorCode,
      txHash: signed.txHash,
    });
    const summary = signed.outcome === "success"
      ? "The owner applied that policy on chain."
      : `The owner transaction did not succeed${signed.errorCode ? ` (${signed.errorCode})` : ""}.`;
    return { chain, summary };
  });
}

async function ownerCall(config, keys, action) {
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
  return signCall({
    rpcUrl: config.rpcUrl,
    signerId: config.accounts.owner,
    secretKey: keys.owner,
    receiverId: config.accounts.wallet,
    actions: [call],
  });
}

export async function ensureAgentActive(config, keys) {
  let agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
    agent_id: config.accounts.agent,
  });
  let restored = null;
  if (!agent || !agent.active) {
    const signed = await ownerCall(config, keys, "restore");
    restored = chainView(config, signed);
    if (signed.outcome !== "success") {
      throw new DemoError(
        "restore_failed",
        `The agent was inactive and the restore did not succeed${signed.errorCode ? ` (${signed.errorCode})` : ""}.`,
        502,
      );
    }
    agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
      agent_id: config.accounts.agent,
    });
  }
  return { agent, restored };
}

export async function withSigningLock(work) {
  if (signing) {
    throw new DemoError("cooldown", "A signature is already in progress.", 429, { retry_after_seconds: 2 });
  }
  signing = true;
  let decision = null;
  try {
    return await work({
      note(next) {
        decision = next;
      },
    });
  } finally {
    signing = false;
    if (decision) await releaseLock(decision.store, decision.lockToken);
  }
}

export function chainView(config, signed) {
  return {
    submitted: true,
    txHash: signed.txHash,
    explorerUrl: signed.txHash ? config.explorerTxBase + signed.txHash : null,
    outcome: signed.outcome,
    errorCode: signed.errorCode,
    rejected: Boolean(signed.rejected),
    detail: signed.detail || null,
  };
}

export async function submitPay(config, keys, attack) {
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

async function assertReviewWillHold(config, attack) {
  if (attack.kind !== "review") return;
  const review = await readReview(config);
  if (!review.supported) {
    throw new DemoError("review_not_enabled", review.reason, 409);
  }
  if (attack.id === "review-payee" && !review.reviewUnknownPayees) {
    throw new DemoError(
      "review_not_enabled",
      "Unknown payees are refused on this wallet, not queued. Queue a payment above the threshold instead. That one is held for approval.",
      409,
    );
  }
  if (attack.id === "above-threshold") {
    const threshold = BigInt(review.approvalThreshold || "0");
    const amount = BigInt(attack.amount);
    if (threshold === 0n || amount <= threshold) {
      throw new DemoError(
        "review_not_enabled",
        "The approval threshold is off or this amount is not above it, so the wallet would pay immediately. This server did not sign.",
        409,
      );
    }
  }
}

export async function runScenario(id, request) {
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  const attack = describeAttack(id, config);
  if (!attack) throw new DemoError("unknown_attack", "unknown scenario", 400);
  await assertTestnet(config.rpcUrl);
  await assertReviewWillHold(config, attack);
  if (signing) {
    throw new DemoError("cooldown", "A signature is already in progress.", 429, { retry_after_seconds: 2 });
  }
  signing = true;
  let decision = null;
  try {
    decision = await gate(config, request);
    let agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
      agent_id: config.accounts.agent,
    });
    let restored = null;
    if (!agent || !agent.active) {
      const signed = await ownerCall(config, keys, "restore");
      restored = chainView(config, signed);
      if (signed.outcome !== "success") {
        throw new DemoError(
          "restore_failed",
          `The agent was inactive and the restore did not succeed${signed.errorCode ? ` (${signed.errorCode})` : ""}.`,
          502,
        );
      }
      agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
        agent_id: config.accounts.agent,
      });
    }
    const chainPayees = agent && Array.isArray(agent.payees) ? agent.payees : undefined;
    const facts = await payeeFacts({
      rpcUrl: config.rpcUrl,
      accountId: attack.payee,
      creation: (config.creationTx && config.creationTx[attack.payee]) || null,
      nowMs: Date.now(),
      indexerBase: config.indexerAccountsBase,
    });
    const riskInputFacts = riskInput(config, facts, chainPayees);
    const risk = classifyPayee(riskInputFacts);
    const amountCheck = classifyAmount({
      amount: attack.amount,
      perCallMax: agent ? agent.per_call_max : config.policy.perCallMax,
      spentToday: agent ? agent.spent_in_window : "0",
      dailyMax: agent ? agent.daily_max : config.policy.dailyMax,
    });
    const chain = { submitted: false, txHash: null, explorerUrl: null, outcome: null, errorCode: null, rejected: false };
    if (shouldSubmit(attack, risk, amountCheck)) {
      const signed = await submitPay(config, keys, attack);
      Object.assign(chain, chainView(config, signed));
      if (attack.kind === "review" && signed.outcome === "success") {
        chain.held = true;
        try {
          chain.pending = await viewCall(config.rpcUrl, config.accounts.wallet, "get_pending", {});
        } catch {
          chain.pending = [];
        }
        chain.paymentId = matchPending(chain.pending, attack.payee, attack.amount);
      }
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
    const summary = explain(attack, risk, amountCheck, chain);
    let telegram = { configured: false, sent: false, reason: "not_held" };
    if (chain.held) {
      try {
        telegram = await notifyHold({
          payee: attack.payee,
          amountDisplay: attack.amountDisplay,
          paymentId: chain.paymentId,
          explorerUrl: chain.explorerUrl,
        });
      } catch {
        telegram = { configured: true, sent: false, reason: "Telegram did not accept the message." };
      }
    }
    await traceOutcome(config, {
      txHash: chain.txHash,
      injection: attack.injection || attack.narrative,
      reasoning: attack.narrative,
      agentMode: "scripted",
      payee: attack.payee,
      amountDisplay: attack.amountDisplay,
      errorCode: chain.errorCode,
      held: chain.held,
      outcome: chain.outcome,
      explorerUrl: chain.explorerUrl,
      checks: [
        { name: "Payee risk", result: risk.verdict },
        { name: "Amount", result: amountCheck.verdict },
        { name: "Wallet", result: chain.held ? "held for the owner" : (chain.errorCode || chain.outcome || "not submitted") },
      ],
    });
    return {
      scenario: scenarioView(attack),
      risk: {
        ...risk,
        ageSource: facts.ageSource,
        exists: facts.exists,
        codeHash: facts.codeHash,
        allowlistSource: riskInputFacts.allowlistSource,
      },
      amountCheck,
      chain,
      restored,
      receipt: receiptFromRun({
        scenario: attack,
        chain,
        amountDisplay: attack.amountDisplay,
        payee: attack.payee,
      }),
      replayUrl: chain.txHash ? `/replay/${chain.txHash}` : null,
      telegram,
      summary: restored
        ? `The agent was revoked, so I restored it before this attempt. ${summary}`
        : summary,
      attemptsToday: decision.attemptsToday,
      limitSource: decision.source,
    };
  } finally {
    signing = false;
    if (decision) await releaseLock(decision.store, decision.lockToken);
  }
}

export async function setSpending(action, request) {
  if (action !== "revoke" && action !== "restore") {
    throw new DemoError("bad_action", "action must be revoke or restore", 400);
  }
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  await assertTestnet(config.rpcUrl);
  const agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
    agent_id: config.accounts.agent,
  });
  const active = Boolean(agent && agent.active);
  if (action === "revoke" && !active) {
    return { action, changed: false, summary: "Spending is already revoked. The key still exists. The next launch restores the agent before it pays, so a revoked agent does not brick the demo." };
  }
  if (action === "restore" && active) {
    return { action, changed: false, summary: "Spending is already enabled." };
  }
  if (signing) {
    throw new DemoError("cooldown", "A signature is already in progress.", 429, { retry_after_seconds: 2 });
  }
  signing = true;
  let decision = null;
  try {
    decision = await gate(config, request);
    const signed = await ownerCall(config, keys, action);
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
        ? "The owner revoked the agent. The key still exists, and the next pay fails closed until the agent is restored."
        : "The owner restored the agent. The day's spend was kept."
      : `The owner transaction did not succeed${signed.errorCode ? ` (${signed.errorCode})` : ""}.`;
    return {
      action,
      changed: signed.outcome === "success",
      chain: chainView(config, signed),
      summary,
    };
  } finally {
    signing = false;
    if (decision) await releaseLock(decision.store, decision.lockToken);
  }
}

export async function decideReview(action, paymentId, request) {
  if (action !== "approve" && action !== "reject") {
    throw new DemoError("bad_action", "action must be approve or reject", 400);
  }
  if (!/^\d+$/.test(String(paymentId || ""))) {
    throw new DemoError("bad_payment", "payment id must be an integer", 400);
  }
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  await assertTestnet(config.rpcUrl);
  const review = await readReview(config);
  if (!review.supported) throw new DemoError("review_not_enabled", review.reason, 409);
  const row = (review.pending || []).find((item) => String(item.payment_id) === String(paymentId));
  const payee = row ? String(row.receiver_id) : null;
  const amount = row && row.amount != null ? String(row.amount) : null;
  let amountDisplay = null;
  if (amount) {
    try {
      amountDisplay = formatAtomic(amount, config.policy.decimals);
    } catch {
      amountDisplay = amount;
    }
  }
  if (signing) {
    throw new DemoError("cooldown", "A signature is already in progress.", 429, { retry_after_seconds: 2 });
  }
  signing = true;
  let decision = null;
  try {
    decision = await gate(config, request);
    const call = await functionCall(
      action === "approve" ? "approve_payment" : "reject_payment",
      { payment_id: String(paymentId) },
      action === "approve" ? 80 : 30,
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
      title: action === "approve" ? "Approve held payment" : "Reject held payment",
      outcome: signed.outcome,
      errorCode: signed.errorCode,
      txHash: signed.txHash,
    });
    const summary = signed.outcome === "success"
      ? action === "approve"
        ? "The owner approved the held payment. The wallet scheduled the transfer."
        : "The owner rejected the held payment. The reserved amount returned to today's budget."
      : `The owner transaction did not succeed${signed.errorCode ? ` (${signed.errorCode})` : ""}.`;
    const chain = chainView(config, signed);
    const receipt = receiptFromRun({
      scenario: {
        narrative: action === "approve"
          ? "The owner approved a held payment."
          : "The owner rejected a held payment.",
        amountDisplay,
        payee,
      },
      chain,
      action,
      amountDisplay,
      payee,
    });
    return { action, paymentId: String(paymentId), chain, summary, receipt, payee, amountDisplay };
  } finally {
    signing = false;
    if (decision) await releaseLock(decision.store, decision.lockToken);
  }
}

export async function restoreIfInactive(request) {
  const config = loadConfig();
  if (!chainReady(config)) return { changed: false, summary: "The demo wallet is not configured." };
  const keys = loadKeys();
  if (!signingReady(keys)) return { changed: false, summary: "Signing keys are not set." };
  await assertTestnet(config.rpcUrl);
  const agent = await viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", {
    agent_id: config.accounts.agent,
  });
  if (agent && agent.active) return { changed: false, summary: "The agent is already active." };
  return setSpending("restore", request);
}
