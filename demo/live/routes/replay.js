import { loadConfig } from "../lib/config.js";
import { json } from "../lib/http.js";
import { loadReplay } from "../lib/replay.js";
import { chainReplay } from "../lib/txread.js";

function prefer(stored, chain) {
  if (!chain) return stored;
  if (!stored) return chain;
  const recordedText = Boolean(stored.injection);
  return {
    ...chain,
    ...stored,
    injection: stored.injection || chain.injection,
    injectionFrom: recordedText ? "recorded" : chain.injectionFrom,
    reasoning: stored.reasoning || chain.reasoning,
    reasoningFrom: stored.reasoning ? "recorded" : "chain",
    payee: stored.payee || chain.payee,
    amountDisplay: stored.amountDisplay || chain.amountDisplay,
    errorCode: stored.errorCode || chain.errorCode,
    ruleText: stored.ruleText || chain.ruleText,
    checks: chain.checks && chain.checks.length ? chain.checks : stored.checks,
    kind: stored.kind && stored.kind !== "unknown" ? stored.kind : chain.kind,
    source: stored.source === "store" ? "store+chain" : "record+chain",
    note: recordedText
      ? "The sentence was recorded when this server signed. The payee, the amount, and the error code are also on the transaction."
      : chain.note,
  };
}

export async function GET(request) {
  const tx = new URL(request.url).searchParams.get("tx") || "";
  if (!/^[A-Za-z0-9]{32,88}$/.test(tx)) {
    return json(404, { error: { code: "not_found", message: "That is not a transaction hash." } });
  }
  const config = loadConfig();
  const stored = await loadReplay(tx);
  let chain = null;
  try {
    chain = await chainReplay(config, tx);
  } catch {
    chain = null;
  }
  const merged = prefer(stored, chain);
  if (!merged || merged.kind === "unknown") {
    return json(404, { error: { code: "not_found", message: "That transaction is not a pay call I can replay." } });
  }
  if (merged.reasoning && /No NEARAI_API_KEY/.test(merged.reasoning)) {
    merged.reasoning = "The agent followed the text in the transaction.";
  }
  delete merged.source;
  return json(200, merged);
}
