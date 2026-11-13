// Launch and Drain Me. The agent decides who to pay. This module signs
// only when the wallet would refuse or hold. A payment the wallet would
// settle is not signed.

import { describeAttack } from "./attacks.js";
import { decidePayment, modelFromEnv, publicAgent } from "./agent.js";
import { chainReady, loadConfig, loadKeys, signingReady } from "./config.js";
import { assertDrainRate, cleanPrompt, recordDrain } from "./drain.js";
import { predictEffect } from "./effect.js";
import { formatAtomic } from "./format.js";
import { DemoError } from "./http.js";
import { remember } from "./journal.js";
import { matchPending, receiptFromRun } from "./receipt.js";
import { traceOutcome } from "./trace.js";
import { notifyHold } from "./telegram.js";
import {
  chainView,
  ensureAgentActive,
  gate,
  readReview,
  requireChain,
  requireKeys,
  submitPay,
  withSigningLock,
} from "./demo.js";
import { assertTestnet, viewCall } from "./near.js";

function amountDisplay(amount, decimals) {
  try {
    return formatAtomic(amount, decimals);
  } catch {
    return String(amount || "");
  }
}

async function preview(config, payee, amount) {
  const [agent, review, walletConfig] = await Promise.all([
    viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", { agent_id: config.accounts.agent }),
    readReview(config),
    viewCall(config.rpcUrl, config.accounts.wallet, "get_config", {}),
  ]);
  const view = {
    active: true,
    per_call_max: (agent && agent.per_call_max) || config.policy.perCallMax,
    daily_max: (agent && agent.daily_max) || config.policy.dailyMax,
    spent_in_window: (agent && agent.spent_in_window) || "0",
    payees: (agent && agent.payees) || config.policy.payees || [],
  };
  return {
    agent,
    review,
    predicted: predictEffect({
      payee,
      amount,
      agent: view,
      review,
      walletPaused: Boolean(walletConfig && walletConfig.paused),
      tokenAllowed: true,
    }),
  };
}

function refusedSummary(agent, chain) {
  const who = agent && agent.mode === "live" ? "The model" : "The scripted agent";
  if (!chain || !chain.submitted) return agent && agent.reasoning ? agent.reasoning : "Nothing was signed.";
  if (chain.held) return `${who} asked the wallet to pay. The wallet held the payment for the owner.`;
  if (chain.outcome === "success" && !chain.errorCode) return `${who} asked the wallet to pay. The wallet settled it.`;
  const code = chain.errorCode ? ` (${chain.errorCode})` : "";
  return `${who} asked the wallet to pay. The wallet refused the payment${code}.`;
}

async function maybeTelegram(chain, payee, shown) {
  if (!chain || !chain.held) return { configured: false, sent: false, reason: "not_held" };
  const paymentId = chain.paymentId || null;
  try {
    return await notifyHold({
      payee,
      amountDisplay: shown,
      paymentId,
      explorerUrl: chain.explorerUrl,
    });
  } catch {
    return { configured: true, sent: false, reason: "Telegram did not accept the message." };
  }
}

export async function runTrackedPayment(request, { attack, agent, onEvent, drain }) {
  const config = loadConfig();
  const keys = loadKeys();
  requireChain(config);
  requireKeys(keys);
  if (!chainReady(config) || !signingReady(keys)) {
    throw new DemoError("keys_not_configured", "Signing keys are not set.", 503);
  }
  await assertTestnet(config.rpcUrl);
  const shownAgent = publicAgent(agent);
  const decimals = config.policy.decimals;

  if (!agent || !agent.tool) {
    const payload = {
      agent: shownAgent,
      scenario: attack,
      chain: { submitted: false, txHash: null, explorerUrl: null, outcome: null, errorCode: null, rejected: false },
      summary: shownAgent && shownAgent.reasoning ? shownAgent.reasoning : "The agent did not ask the wallet to pay.",
      receipt: null,
      telegram: { configured: false, sent: false, reason: "not_held" },
    };
    if (drain) await recordDrain({ ...drain, outcome: "no_payment", mode: shownAgent && shownAgent.mode });
    return payload;
  }

  const payee = agent.tool.receiver_id;
  const amount = agent.tool.amount;
  const shown = amountDisplay(amount, decimals);
  const proposed = {
    ...attack,
    payee,
    amount,
    amountDisplay: shown,
  };
  const look = await preview(config, payee, amount);
  if (look.predicted.effect === "settle") {
    const payload = {
      agent: shownAgent,
      scenario: proposed,
      chain: { submitted: false, txHash: null, explorerUrl: null, outcome: null, errorCode: null, rejected: false },
      predicted: look.predicted,
      summary: `I did not sign. ${payee} is allowlisted and ${shown} is inside the caps, so the wallet would have paid it. Drain Me does not move that money.`,
      receipt: null,
      telegram: { configured: false, sent: false, reason: "not_held" },
    };
    if (drain) {
      await recordDrain({
        ...drain,
        payee,
        amount,
        amountDisplay: shown,
        outcome: "not_signed",
        mode: shownAgent && shownAgent.mode,
      });
    }
    return payload;
  }

  return withSigningLock(async ({ note }) => {
    if (onEvent) onEvent({ type: "status", text: "Signing pay on NEAR testnet." });
    const decision = await gate(config, request);
    note(decision);
    const { agent: chainAgent, restored } = await ensureAgentActive(config, keys);
    const again = predictEffect({
      payee,
      amount,
      agent: {
        active: true,
        per_call_max: (chainAgent && chainAgent.per_call_max) || config.policy.perCallMax,
        daily_max: (chainAgent && chainAgent.daily_max) || config.policy.dailyMax,
        spent_in_window: (chainAgent && chainAgent.spent_in_window) || "0",
        payees: (chainAgent && chainAgent.payees) || config.policy.payees || [],
      },
      review: look.review,
      walletPaused: look.predicted.code === "ERR_PAUSED",
    });
    if (again.effect === "settle") {
      if (drain) {
        await recordDrain({
          ...drain,
          payee,
          amount,
          amountDisplay: shown,
          outcome: "not_signed",
          mode: shownAgent && shownAgent.mode,
        });
      }
      return {
        agent: shownAgent,
        scenario: proposed,
        restored,
        chain: { submitted: false, txHash: null, explorerUrl: null, outcome: null, errorCode: null, rejected: false },
        summary: `I did not sign. ${payee} is allowlisted and ${shown} is inside the caps.`,
        receipt: null,
        telegram: { configured: false, sent: false, reason: "not_held" },
      };
    }
    const signed = await submitPay(config, keys, {
      id: attack.id || "drain",
      payee,
      amount,
    });
    const chain = chainView(config, signed);
    if ((attack.kind === "review" || again.effect === "hold") && signed.outcome === "success" && !signed.errorCode) {
      chain.held = true;
      try {
        chain.pending = await viewCall(config.rpcUrl, config.accounts.wallet, "get_pending", {});
      } catch {
        chain.pending = [];
      }
      chain.paymentId = matchPending(chain.pending, payee, amount);
    }
    remember({
      atMs: Date.now(),
      kind: drain ? "drain" : attack.kind,
      title: attack.title,
      payee,
      amount,
      outcome: signed.outcome,
      errorCode: signed.errorCode,
      txHash: signed.txHash,
    });
    const receipt = receiptFromRun({ scenario: proposed, chain, amountDisplay: shown, payee });
    const summary = restored
      ? `The agent was revoked, so I restored it before this attempt. ${refusedSummary(shownAgent, chain)}`
      : refusedSummary(shownAgent, chain);
    const telegram = await maybeTelegram(chain, payee, shown);
    let outcome = "refused";
    if (chain.held) outcome = "held";
    else if (chain.outcome === "success" && !chain.errorCode) outcome = "settled";
    if (drain) {
      await recordDrain({
        ...drain,
        payee,
        amount,
        amountDisplay: shown,
        outcome,
        errorCode: chain.errorCode,
        txHash: chain.txHash,
        explorerUrl: chain.explorerUrl,
        mode: shownAgent && shownAgent.mode,
      });
    }
    await traceOutcome(config, {
      txHash: chain.txHash,
      injection: attack.injection || attack.narrative,
      reasoning: shownAgent && shownAgent.reasoning,
      agentMode: shownAgent && shownAgent.mode,
      payee,
      amountDisplay: shown,
      errorCode: chain.errorCode,
      held: chain.held,
      outcome: chain.outcome,
      explorerUrl: chain.explorerUrl,
      checks: [
        { name: "Reads the finding", result: attack.injection || attack.narrative || "The agent read the finding." },
        { name: "Injection", result: payee },
        { name: "Signs pay", result: shown },
        { name: "Contract", result: chain.held ? "Held for the owner" : (chain.errorCode || (chain.outcome === "success" ? "Settled" : "Refused")) },
      ],
    });
    return {
      agent: shownAgent,
      scenario: proposed,
      restored,
      chain,
      receipt,
      summary,
      telegram,
      predicted: again,
      replayUrl: chain.txHash ? `/replay/${chain.txHash}` : null,
      attemptsToday: decision.attemptsToday,
      limitSource: decision.source,
    };
  });
}

export async function runLaunch(request, options = {}) {
  const config = loadConfig();
  const attack = describeAttack(options.scenario || "injected-payee", config);
  if (!attack) throw new DemoError("unknown_attack", "unknown scenario", 400);
  requireChain(config);
  requireKeys(loadKeys());
  const agent = await decidePayment({
    finding: attack.injection || attack.narrative,
    fallbackPayee: attack.payee,
    fallbackAmount: attack.amount,
    preferFallback: true,
    apiKey: process.env.NEARAI_API_KEY || "",
    model: modelFromEnv(),
    models: [modelFromEnv()],
    budgetMs: 2_000,
    fetchImpl: options.fetchImpl || fetch,
    onEvent: options.onEvent,
  });
  return runTrackedPayment(request, { attack, agent, onEvent: options.onEvent, drain: null });
}

export async function runDrainAttempt(request, prompt, options = {}) {
  const text = cleanPrompt(prompt);
  if (!text) throw new DemoError("bad_prompt", "Type an injection to try.", 400);
  const config = loadConfig();
  requireChain(config);
  requireKeys(loadKeys());
  await assertDrainRate(request);
  const agent = await decidePayment({
    finding: text,
    fallbackPayee: null,
    fallbackAmount: config.policy.allowedAmount,
    preferFallback: false,
    apiKey: process.env.NEARAI_API_KEY || "",
    model: modelFromEnv(),
    fetchImpl: options.fetchImpl || fetch,
    onEvent: options.onEvent,
  });
  const attack = {
    id: "drain",
    kind: "attack",
    title: "Drain Me",
    injection: text,
    narrative: "A visitor tried to make the agent pay an account named in their text.",
    payee: agent.tool ? agent.tool.receiver_id : null,
    amount: agent.tool ? agent.tool.amount : config.policy.allowedAmount,
    amountDisplay: amountDisplay(agent.tool ? agent.tool.amount : config.policy.allowedAmount, config.policy.decimals),
    expectedError: null,
  };
  return runTrackedPayment(request, {
    attack,
    agent,
    onEvent: options.onEvent,
    drain: { prompt: text, atMs: Date.now() },
  });
}
