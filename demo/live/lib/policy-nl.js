// Natural-language policy. A deterministic parser always works. A model
// may propose the same shape, and its output is kept only when it matches
// the schema. Nothing here signs a transaction.

import { createHash } from "node:crypto";
import { isNearAccount } from "./accounts.js";
import { formatAmount } from "./format.js";
import { modelFromEnv, MODEL_URL } from "./agent.js";

const ATOMIC = /^[0-9]{1,40}$/;
const FIELDS = ["dailyMaxAtomic", "perCallMaxAtomic", "payees", "approvalThresholdAtomic", "reviewUnknownPayees"];

export function decimalToAtomic(text, decimals) {
  const match = /^(\d+)(?:\.(\d+))?$/.exec(String(text || "").trim());
  if (!match) return null;
  const places = Number(decimals);
  if (!Number.isInteger(places) || places < 0 || places > 36) return null;
  const frac = (match[2] || "").slice(0, places).padEnd(places, "0");
  try {
    const scale = 10n ** BigInt(places);
    const value = BigInt(match[1]) * scale + (frac ? BigInt(frac) : 0n);
    return value.toString();
  } catch {
    return null;
  }
}

export function parsePolicyText(text, context) {
  const source = String(text || "");
  const decimals = context.decimals;
  const notes = [];
  const out = {
    dailyMaxAtomic: null,
    perCallMaxAtomic: null,
    payees: null,
    approvalThresholdAtomic: null,
    reviewUnknownPayees: null,
  };
  const day = source.match(/(?:max(?:imum)?|cap|limit)\s+(\d+(?:\.\d+)?)\s*(?:wnear|near)\s+(?:a|per)\s+day/i);
  const call = source.match(/(?:max(?:imum)?|cap|limit)\s+(\d+(?:\.\d+)?)\s*(?:wnear|near)\s+(?:per|each|a)\s+call/i);
  const ask = source.match(/ask me above\s+(\d+(?:\.\d+)?)/i);
  const named = source.match(/only pay\s+([a-z0-9][a-z0-9._-]*\.(?:near|testnet|tg))/i);
  const atomic = (text) => {
    const value = decimalToAtomic(text, decimals);
    return value != null && ATOMIC.test(value) ? value : null;
  };
  if (day) out.dailyMaxAtomic = atomic(day[1]);
  if (call) out.perCallMaxAtomic = atomic(call[1]);
  if (ask) out.approvalThresholdAtomic = atomic(ask[1]);
  if (/only pay the triage api/i.test(source) && context.triagePayee && isNearAccount(context.triagePayee)) {
    out.payees = [context.triagePayee.toLowerCase()];
    notes.push("I read \"the triage API\" as the demo payee.");
  } else if (named && isNearAccount(named[1])) {
    out.payees = [named[1].toLowerCase()];
  }
  if (/hold unknown payees|queue unknown payees|ask me about new payees/i.test(source)) {
    out.reviewUnknownPayees = true;
  }
  const any = FIELDS.some((key) => out[key] != null);
  return { ok: any, proposal: out, notes, mode: "parser" };
}

export function validatePolicy(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { ok: false, reason: "The model output was not an object." };
  }
  for (const key of Object.keys(value)) {
    if (!FIELDS.includes(key)) return { ok: false, reason: `Unknown field ${key}.` };
  }
  const proposal = {
    dailyMaxAtomic: null,
    perCallMaxAtomic: null,
    payees: null,
    approvalThresholdAtomic: null,
    reviewUnknownPayees: null,
  };
  for (const key of ["dailyMaxAtomic", "perCallMaxAtomic", "approvalThresholdAtomic"]) {
    if (value[key] == null) continue;
    if (typeof value[key] !== "string" || !ATOMIC.test(value[key])) {
      return { ok: false, reason: `${key} must be an atomic integer string.` };
    }
    proposal[key] = value[key];
  }
  if (value.payees != null) {
    if (!Array.isArray(value.payees) || value.payees.length > 8) {
      return { ok: false, reason: "payees must be a list of at most 8 accounts." };
    }
    const payees = [];
    for (const id of value.payees) {
      if (!isNearAccount(String(id || ""))) return { ok: false, reason: "A payee is not a NEAR account." };
      payees.push(String(id).toLowerCase());
    }
    proposal.payees = payees;
  }
  if (value.reviewUnknownPayees != null) {
    if (typeof value.reviewUnknownPayees !== "boolean") {
      return { ok: false, reason: "reviewUnknownPayees must be true or false." };
    }
    proposal.reviewUnknownPayees = value.reviewUnknownPayees;
  }
  return { ok: true, proposal };
}

export function diffPolicy(current, proposal, decimals, symbol) {
  const changes = [];
  const show = (atomic) => (atomic == null ? "unchanged" : formatAmount(atomic, decimals, symbol));
  if (proposal.dailyMaxAtomic && proposal.dailyMaxAtomic !== current.dailyMaxAtomic) {
    changes.push({ field: "daily max", from: show(current.dailyMaxAtomic), to: show(proposal.dailyMaxAtomic) });
  }
  if (proposal.perCallMaxAtomic && proposal.perCallMaxAtomic !== current.perCallMaxAtomic) {
    changes.push({ field: "per-call max", from: show(current.perCallMaxAtomic), to: show(proposal.perCallMaxAtomic) });
  }
  if (proposal.approvalThresholdAtomic && proposal.approvalThresholdAtomic !== current.approvalThresholdAtomic) {
    changes.push({ field: "approval threshold", from: show(current.approvalThresholdAtomic), to: show(proposal.approvalThresholdAtomic) });
  }
  if (proposal.payees) {
    const next = proposal.payees.join(", ");
    const prev = (current.payees || []).join(", ");
    if (next !== prev) changes.push({ field: "allowlist", from: prev || "empty", to: next });
  }
  if (proposal.reviewUnknownPayees != null && proposal.reviewUnknownPayees !== current.reviewUnknownPayees) {
    changes.push({
      field: "unknown payees",
      from: current.reviewUnknownPayees ? "queued" : "refused",
      to: proposal.reviewUnknownPayees ? "queued" : "refused",
    });
  }
  return changes;
}

/**
 * Applying a policy signs with the owner key. It needs the flag AND an
 * owner-action token; the public page never has the token, so on the
 * public demo apply is off whatever the flag says (red-team RT-03).
 */
export function applyAllowed(env = process.env) {
  return String(env.TRIAGEPAY_POLICY_APPLY || "") === "1"
    && String(env.TRIAGEPAY_OWNER_ACTION_TOKEN || "").trim().length >= 16;
}

/** A stable digest of the exact change set a preview showed. */
export function changesDigest(changes, proposal) {
  const canonical = JSON.stringify({
    changes: (changes || []).map((row) => [row.field, String(row.from), String(row.to)]),
    proposal: Object.keys(proposal || {}).sort().map((key) => [key, proposal[key]]),
  });
  return createHash("sha256").update(canonical).digest("hex");
}

/**
 * Hard bounds a signed policy must satisfy whatever the model wrote:
 * payees from the committed allowlist only, caps no looser than the
 * committed ones. Returns a reason string, or null when within bounds.
 */
export function outOfBounds(config, next) {
  const allowed = new Set((config.policy.payees || []).map((item) => String(item).toLowerCase()));
  for (const payee of next.payees || []) {
    if (!allowed.has(String(payee).toLowerCase())) return `payee ${payee} is not on the committed allowlist`;
  }
  if (BigInt(next.perCallMax) > BigInt(config.policy.perCallMax)) return "the per-call cap would exceed the committed ceiling";
  if (BigInt(next.dailyMax) > BigInt(config.policy.dailyMax)) return "the daily cap would exceed the committed ceiling";
  return null;
}

export async function proposePolicy({ text, context, apiKey, fetchImpl = fetch, env = process.env }) {
  const parsed = parsePolicyText(text, context);
  if (!String(apiKey || "").trim()) {
    return { ...parsed, modelNote: null };
  }
  try {
    const response = await fetchImpl(MODEL_URL, {
      method: "POST",
      headers: {
        authorization: `Bearer ${apiKey}`,
        "content-type": "application/json",
        "x-no-aliasing": "true",
      },
      body: JSON.stringify({
        model: modelFromEnv(env),
        temperature: 0,
        max_tokens: 300,
        messages: [
          {
            role: "system",
            content: "Turn the owner's sentence into JSON with only these keys: dailyMaxAtomic, perCallMaxAtomic, payees, approvalThresholdAtomic, reviewUnknownPayees. Amounts are integer atomic strings. Omit a key you cannot fill. No prose.",
          },
          { role: "user", content: String(text || "").slice(0, 500) },
        ],
      }),
      signal: AbortSignal.timeout(45_000),
    });
    if (!response.ok) {
      try { await response.body?.cancel?.(); } catch { /* status only */ }
      return { ...parsed, modelNote: `The model call failed (HTTP ${response.status}), so I used the parser.` };
    }
    const body = await response.json();
    const content = body?.choices?.[0]?.message?.content || "";
    let value = null;
    try {
      value = JSON.parse(content);
    } catch {
      return { ...parsed, modelNote: "The model did not return JSON, so I used the parser." };
    }
    const checked = validatePolicy(value);
    if (!checked.ok) {
      return { ...parsed, modelNote: `${checked.reason} I used the parser.` };
    }
    return { ok: true, proposal: checked.proposal, notes: [], mode: "live", modelNote: null };
  } catch {
    return { ...parsed, modelNote: "The model call failed, so I used the parser." };
  }
}
