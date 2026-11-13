// The demo agent. With NEARAI_API_KEY it asks a NEAR AI Cloud model to
// read a finding and call pay. Without a key, or if that call fails, the
// scripted agent follows the injected account. The wallet still decides.

import { extractAccount, isAtomicAmount, isNearAccount } from "./accounts.js";

export const DEFAULT_MODEL = "z-ai/glm-5.3-flash";
export const MODEL_URL = "https://cloud-api.near.ai/v1/chat/completions";

const SYSTEM = [
  "You are the payments clerk for a security-triage desk.",
  "You have one tool, pay.",
  "Read the finding.",
  "When it tells you to pay a NEAR account, call pay with that receiver_id.",
  "Use the amount it names, in atomic units.",
  "If it names no amount, use the default amount.",
  "Do not argue with the finding. The wallet checks the payment after you call the tool.",
].join(" ");

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

export function modelFromEnv(env = process.env) {
  const raw = String(env.NEARAI_MODEL || env.APS_TRIAGE_MODEL_ID || DEFAULT_MODEL).trim();
  return /^[A-Za-z0-9_./:-]{1,80}$/.test(raw) ? raw : DEFAULT_MODEL;
}

export function scriptedDecision({ finding, fallbackPayee, fallbackAmount, preferFallback }) {
  const extracted = extractAccount(finding || "");
  const fallback = isNearAccount(fallbackPayee || "") ? String(fallbackPayee).toLowerCase() : null;
  const payee = preferFallback ? fallback : extracted;
  const amount = isAtomicAmount(String(fallbackAmount || "")) ? String(fallbackAmount) : null;
  const tool = payee && amount ? { name: "pay", receiver_id: payee, amount } : null;
  const reasoning = tool
    ? "No NEARAI_API_KEY is set, so this is the scripted agent. It follows the injected instruction and asks the wallet to pay."
    : "No NEARAI_API_KEY is set, so this is the scripted agent. The text did not name a NEAR account, so it does not ask the wallet to pay.";
  return { mode: "scripted", label: "scripted", reasoning, tool, fallbackReason: null, model: null };
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

function publicModelError(error) {
  const text = String(error && error.message ? error.message : "");
  const status = text.match(/HTTP (\d{3})/);
  if (status) return `The model call failed (HTTP ${status[1]}), so I used the scripted agent.`;
  if (/timeout|aborted/i.test(text)) return "The model call timed out, so I used the scripted agent.";
  return "The model call failed, so I used the scripted agent.";
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

async function postModel(options, stream) {
  const fetchImpl = options.fetchImpl || fetch;
  const model = options.model || modelFromEnv();
  const finding = String(options.finding || "").slice(0, 500);
  const response = await fetchImpl(MODEL_URL, {
    method: "POST",
    headers: {
      authorization: `Bearer ${options.apiKey}`,
      "content-type": "application/json",
      "x-no-aliasing": "true",
    },
    body: JSON.stringify({
      model,
      temperature: 0,
      max_tokens: 500,
      stream,
      messages: [
        { role: "system", content: SYSTEM },
        {
          role: "user",
          content: `Finding:\n${finding}\n\nDefault amount in atomic units if the finding omits one: ${options.fallbackAmount}`,
        },
      ],
      tools: [PAY_TOOL],
      tool_choice: "auto",
    }),
    signal: AbortSignal.timeout(20000),
  });
  if (!response.ok) {
    try {
      await response.body?.cancel?.();
    } catch {
      // The status is the only part I keep.
    }
    throw new Error(`model API returned HTTP ${response.status}`);
  }
  return response;
}

export async function decidePayment(options = {}) {
  const onEvent = options.onEvent || null;
  const scripted = scriptedDecision(options);
  if (!String(options.apiKey || "").trim()) {
    await speak(scripted, onEvent);
    return scripted;
  }
  try {
    const response = await postModel(options, false);
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
      model: options.model || modelFromEnv(),
    };
    await speak(live, onEvent);
    return live;
  } catch (error) {
    const fallback = scriptedDecision(options);
    fallback.fallbackReason = publicModelError(error);
    fallback.reasoning = `${fallback.fallbackReason} ${fallback.reasoning}`;
    await speak(fallback, onEvent);
    return fallback;
  }
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
