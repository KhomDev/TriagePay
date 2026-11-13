import { loadConfig, loadKeys, chainReady, signingReady } from "../lib/config.js";
import { DemoError, fromError, json, readJson } from "../lib/http.js";
import { formatAmount, tokenSymbol } from "../lib/format.js";
import { diffPolicy, parsePolicyText, proposePolicy, applyAllowed } from "../lib/policy-nl.js";
import { readReview, signOwnerPolicy } from "../lib/demo.js";
import { viewCall } from "../lib/near.js";

function contextFrom(config) {
  return {
    decimals: config.policy.decimals,
    triagePayee: config.accounts.payee,
    symbol: tokenSymbol(config.accounts.token),
  };
}

export async function POST(request) {
  try {
    const body = await readJson(request);
    const text = typeof body.text === "string" ? body.text.trim() : "";
    if (!text || text.length > 500) throw new DemoError("bad_policy", "Type a policy in one sentence, up to 500 characters.", 400);
    const action = body.action === "apply" ? "apply" : "preview";
    if (action === "apply" && !applyAllowed()) {
      throw new DemoError(
        "apply_disabled",
        "Applying a policy is off on this public demo. This preview does not sign a transaction.",
        403,
      );
    }
    const config = loadConfig();
    const ctx = contextFrom(config);
    const proposed = await proposePolicy({
      text,
      context: ctx,
      apiKey: body.fast ? "" : (process.env.NEARAI_API_KEY || ""),
      env: process.env,
    });
    if (!chainReady(config)) {
      return json(200, {
        ...proposed,
        changes: [],
        applyAllowed: applyAllowed(),
        note: "The demo wallet is not configured, so I cannot diff this against a chain policy.",
      });
    }
    const [agent, review] = await Promise.all([
      viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", { agent_id: config.accounts.agent }),
      readReview(config),
    ]);
    const current = {
      dailyMaxAtomic: agent ? String(agent.daily_max) : String(config.policy.dailyMax),
      perCallMaxAtomic: agent ? String(agent.per_call_max) : String(config.policy.perCallMax),
      payees: agent && Array.isArray(agent.payees) ? agent.payees.map(String) : (config.policy.payees || []),
      approvalThresholdAtomic: review && review.supported ? String(review.approvalThreshold || "0") : "0",
      reviewUnknownPayees: Boolean(review && review.reviewUnknownPayees),
    };
    const changes = diffPolicy(current, proposed.proposal, ctx.decimals, ctx.symbol);
    const nextPer = proposed.proposal.perCallMaxAtomic || current.perCallMaxAtomic;
    const nextDay = proposed.proposal.dailyMaxAtomic || current.dailyMaxAtomic;
    if (BigInt(nextPer) > BigInt(nextDay)) {
      throw new DemoError("policy_inconsistent", "The per-call cap would be above the daily cap. I did not sign.", 400);
    }
    if (action === "preview" || !changes.length) {
      return json(200, {
        mode: proposed.mode,
        notes: proposed.notes,
        modelNote: proposed.modelNote,
        proposal: {
          ...proposed.proposal,
          dailyDisplay: proposed.proposal.dailyMaxAtomic ? formatAmount(proposed.proposal.dailyMaxAtomic, ctx.decimals, ctx.symbol) : null,
          perCallDisplay: proposed.proposal.perCallMaxAtomic ? formatAmount(proposed.proposal.perCallMaxAtomic, ctx.decimals, ctx.symbol) : null,
          thresholdDisplay: proposed.proposal.approvalThresholdAtomic ? formatAmount(proposed.proposal.approvalThresholdAtomic, ctx.decimals, ctx.symbol) : null,
        },
        changes,
        applyAllowed: applyAllowed(),
        signed: false,
      });
    }
    const keys = loadKeys();
    if (!signingReady(keys)) throw new DemoError("keys_not_configured", "The owner key is not set, so I cannot apply this.", 503);
    const signed = await signOwnerPolicy(request, {
      setAgent: (proposed.proposal.dailyMaxAtomic || proposed.proposal.perCallMaxAtomic || proposed.proposal.payees)
        ? {
          perCallMax: nextPer,
          dailyMax: nextDay,
          payees: proposed.proposal.payees || current.payees,
        }
        : null,
      setReview: (proposed.proposal.approvalThresholdAtomic || proposed.proposal.reviewUnknownPayees != null)
        ? {
          approvalThreshold: proposed.proposal.approvalThresholdAtomic || current.approvalThresholdAtomic,
          reviewUnknownPayees: proposed.proposal.reviewUnknownPayees == null
            ? current.reviewUnknownPayees
            : proposed.proposal.reviewUnknownPayees,
        }
        : null,
    });
    return json(200, { mode: proposed.mode, changes, applyAllowed: true, signed: true, ...signed });
  } catch (error) {
    return fromError(error);
  }
}
