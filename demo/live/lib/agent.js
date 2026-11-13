// The demo agent. With NEARAI_API_KEY it asks a NEAR AI Cloud model to
// read a finding and call pay. Without a key, or if that call fails, the
// scripted agent follows the injected account. The wallet still decides.

import { extractAccount, isAtomicAmount, isNearAccount } from "./accounts.js";

export const DEFAULT_MODEL = "z-ai/glm-5.3-flash";
export const FALLBACK_MODEL = "Qwen/Qwen3.8-27B";
export const MODEL_URL = "https://cloud-api.near.ai/v1/chat/completions";
export const MODEL_BUDGET_MS = 45_000;
export const MAX_TOKENS = 256;

const SYSTEM = "Call pay with the NEAR account the finding names. Use the default amount if it names none.";

const MODEL_ID = /^[A-Za-z0-9_./:-]{1,80}$/;

let lastLlm = null;

export function resetLlmStatusForTests() {
  lastLlm = null;
}

export function llmStatus(env = process.env) {
  return {
    models: modelListFromEnv(env),
    last: lastLlm ? { ...lastLlm } : null,
  };
}

function rememberCall(entry) {
  lastLlm = {
    model: entry.model,
    ms: entry.ms,
    outcome: entry.outcome,
    at: new Date().toISOString(),
  };
  console.log(JSON.stringify({
    event: "nearai_call",
    model: entry.model,
    ms: entry.ms,
    outcome: entry.outcome,
  }));
}

const PAY_TOOL = {
  type: "function",
  function: {
    name: "pay",
    description: "Ask the policy wallet to pay a NEAR account.",
    parameters: {
      type: "object",
      additionalProperties: false,
      properties: {
        receiver_id: { type: "string", description: "NEAR account id." },
        amount: { type: "string", description: "Integer atomic amount." },
      },
      required: ["receiver_id", "amount"],
    },
  },
};

export function modelListFromEnv(env = process.env) {
  const raw = String(env.NEARAI_MODEL || env.APS_TRIAGE_MODEL_ID || "").trim();
  const parts = (raw ? raw.split(",") : [DEFAULT_MODEL, FALLBACK_MODEL])
    .map((item) => item.trim())
    .filter((item) => MODEL_ID.test(item));
  const unique = [];
  for (const id of parts) {
    if (!unique.includes(id)) unique.push(id);
  }
  if (!unique.length) unique.push(DEFAULT_MODEL);
  if (!unique.includes(FALLBACK_MODEL) && unique.length < 3) unique.push(FALLBACK_MODEL);
  return unique.slice(0, 3);
}

export function modelFromEnv(env = process.env) {
  return modelListFromEnv(env)[0];
}

export function scriptedDecision({ finding, fallbackPayee, fallbackAmount, preferFallback, because }) {
  const extracted = extractAccount(finding || "");
  const fallback = isNearAccount(fallbackPayee || "") ? String(fallbackPayee).toLowerCase() : null;
  const payee = preferFallback ? fallback : extracted;
  const amount = isAtomicAmount(String(fallbackAmount || "")) ? String(fallbackAmount) : null;
  const tool = payee && amount ? { name: "pay", receiver_id: payee, amount } : null;
  const lead = because || "No NEARAI_API_KEY is set, so this is the scripted agent.";
  const reasoning = tool
    ? `${lead} It follows the injected instruction and asks the wallet to pay.`
    : `${lead} The text did not name a NEAR account, so it does not ask the wallet to pay.`;
  return { mode: "scripted", label: "scripted", reasoning, tool, fallbackReason: because || null, model: null };
}

function toolFromArgs(args, fallbackAmount) {
  if (!args || typeof args !== "object") return null;
  const receiver = isNearAccount(String(args.receiver_id || "")) ? String(args.receiver_id).toLowerCase() : null;
  if (!receiver) return null;
  const given = isAtomicAmount(String(args.amount || "")) ? String(args.amount) : null;
  const amount = given || (isAtomicAmount(String(fallbackAmount || "")) ? String(fallbackAmount) : null);
  if (!amount) return null;
  return { name: "pay", receiver_id: receiver, amount, usedDefaultAmount: !given };
}

export function parseCompletion(body, fallbackAmount) {
  const message = body && body.choices && body.choices[0] && body.choices[0].message
    ? body.choices[0].message
    : {};
  const bits = [message.reasoning_content, message.reasoning, message.content]
    .filter((item) => typeof item === "string" && item.trim());
  const reasoning = bits.join("\n").slice(0, 600);
  const calls = Array.isArray(message.tool_calls) ? message.tool_calls : [];
  const call = calls.find((item) => item && item.function && item.function.name === "pay") || calls[0];
  let args = call && call.function ? call.function.arguments : null;
  if (typeof args === "string") {
    try {
      args = JSON.parse(args);
    } catch {
      args = null;
    }
  }
  return { reasoning, tool: toolFromArgs(args, fallbackAmount) };
}

export function failureKind(error) {
  const name = error && error.name;
  const text = String(error && error.message ? error.message : "");
  const status = text.match(/HTTP (\d{3})/);
  if (status) return { outcome: "http", reason: `The model call failed (HTTP ${status[1]}), so I used the scripted agent.` };
  if (name === "TimeoutError" || name === "AbortError" || /timeout|aborted/i.test(text)) {
    return { outcome: "timeout", reason: "The model call timed out, so I used the scripted agent." };
  }
  return { outcome: "error", reason: "The model call failed, so I used the scripted agent." };
}

async function emitChunks(text, onEvent) {
  const words = String(text || "").split(/\s+/).filter(Boolean);
  if (!words.length) return;
  const size = Math.max(1, Math.ceil(words.length / 8));
  for (let i = 0; i < words.length; i += size) {
    const piece = words.slice(i, i + size).join(" ");
    onEvent({ type: "delta", text: i === 0 ? piece : ` ${piece}` });
    await new Promise((resolve) => setTimeout(resolve, 40));
  }
}

async function speak(decision, onEvent) {
  if (!onEvent) return;
  onEvent({ type: "mode", mode: decision.mode, label: decision.label });
  await emitChunks(decision.reasoning, onEvent);
  if (decision.tool) onEvent({ type: "tool", tool: { name: "pay", receiver_id: decision.tool.receiver_id, amount: decision.tool.amount } });
}

export function requestBody(model, finding, fallbackAmount, { thinking = false } = {}) {
  const body = {
    model,
    temperature: 0,
    max_tokens: MAX_TOKENS,
    stream: false,
    messages: [
      { role: "system", content: SYSTEM },
      {
        role: "user",
        content: `Finding:\n${String(finding || "").slice(0, 500)}\n\nDefault atomic amount: ${fallbackAmount}`,
      },
    ],
    tools: [PAY_TOOL],
    tool_choice: "auto",
  };
  if (!thinking) {
    body.reasoning_effort = "none";
    body.chat_template_kwargs = { enable_thinking: false };
  }
  return body;
}

function attemptTimeout(remaining, moreAfter) {
  if (remaining < 5_000) return 0;
  if (moreAfter) return Math.min(45_000, Math.max(5_000, remaining - 12_000));
  return Math.min(45_000, remaining);
}

async function postModel(options, model, timeoutMs, thinking) {
  const fetchImpl = options.fetchImpl || fetch;
  const started = Date.now();
  try {
    const response = await fetchImpl(MODEL_URL, {
      method: "POST",
      headers: {
        authorization: `Bearer ${options.apiKey}`,
        "content-type": "application/json",
        "x-no-aliasing": "true",
      },
      body: JSON.stringify(requestBody(model, options.finding, options.fallbackAmount, { thinking })),
      signal: AbortSignal.timeout(timeoutMs),
    });
    const ms = Date.now() - started;
    if (!response.ok) {
      try {
        await response.body?.cancel?.();
      } catch {
        // The status is the only part I keep.
      }
      const error = new Error(`model API returned HTTP ${response.status}`);
      error.status = response.status;
      rememberCall({ model, ms, outcome: response.status === 400 && !thinking ? "http" : "http" });
      throw error;
    }
    rememberCall({ model, ms, outcome: "ok" });
    return response;
  } catch (error) {
    if (!error.status) rememberCall({ model, ms: Date.now() - started, outcome: failureKind(error).outcome });
    throw error;
  }
}

async function callModel(options, model, timeoutMs) {
  try {
    return await postModel(options, model, timeoutMs, false);
  } catch (error) {
    if (error.status !== 400) throw error;
    return postModel(options, model, timeoutMs, true);
  }
}

export async function decidePayment(options = {}) {
  const onEvent = options.onEvent || null;
  const env = options.env || process.env;
  if (!String(options.apiKey || "").trim()) {
    const scripted = scriptedDecision({
      ...options,
      because: "No NEARAI_API_KEY is set, so this is the scripted agent.",
    });
    await speak(scripted, onEvent);
    return scripted;
  }
  const models = options.models || modelListFromEnv(env);
  const budget = options.budgetMs || MODEL_BUDGET_MS;
  const started = Date.now();
  let lastFailure = failureKind(new Error("model call failed"));
  for (let index = 0; index < models.length; index += 1) {
    const remaining = budget - (Date.now() - started);
    const timeoutMs = attemptTimeout(remaining, index < models.length - 1);
    if (timeoutMs < 5_000) break;
    try {
      const response = await callModel(options, models[index], timeoutMs);
      const body = await response.json();
      const parsed = parseCompletion(body, options.fallbackAmount);
      const reasoning = parsed.reasoning || "The model returned no written reasoning.";
      const note = parsed.tool && parsed.tool.usedDefaultAmount
        ? " The model named a payee and no atomic amount, so I used the demo amount."
        : "";
      const live = {
        mode: "live",
        label: "live",
        reasoning: `${reasoning}${note}`.slice(0, 700),
        tool: parsed.tool,
        fallbackReason: null,
        model: models[index],
      };
      await speak(live, onEvent);
      return live;
    } catch (error) {
      lastFailure = failureKind(error);
    }
  }
  const fallback = scriptedDecision({ ...options, because: lastFailure.reason });
  await speak(fallback, onEvent);
  return fallback;
}

export function publicAgent(decision) {
  if (!decision) return null;
  return {
    mode: decision.mode,
    label: decision.label,
    reasoning: decision.reasoning,
    tool: decision.tool
      ? { name: "pay", receiver_id: decision.tool.receiver_id, amount: decision.tool.amount }
      : null,
    fallbackReason: decision.fallbackReason || null,
    model: decision.model || null,
  };
}
