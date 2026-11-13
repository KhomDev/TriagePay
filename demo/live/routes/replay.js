import { loadConfig } from "../lib/config.js";
import { json } from "../lib/http.js";
import { loadReplay, partialReplay } from "../lib/replay.js";
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
    checks: stored.checks && stored.checks.length ? stored.checks : chain.checks,
    kind: stored.kind && stored.kind !== "unknown" ? stored.kind : chain.kind,
    source: stored.source === "store" ? "store+chain" : "record+chain",
    note: recordedText
      ? "The sentence was recorded when this server signed. The payee, the amount, and the error code are also on the transaction."
      : chain.note,
  };
}

export async function GET(request) {
  const tx = new URL(request.url).searchParams.get("tx") || "";
  const config = loadConfig();
  const stored = await loadReplay(tx);
  let chain = null;
  try {
    chain = await chainReplay(config, tx);
  } catch {
    chain = null;
  }
  const merged = prefer(stored, chain);
  if (merged) return json(200, merged);
  const partial = partialReplay(tx, config.explorerTxBase);
  if (!partial) return json(400, { error: { code: "bad_tx", message: "That transaction hash is not usable." } });
  partial.note = "The RPC did not return this transaction, so I only have the hash. The explorer link is the chain record.";
  return json(200, partial);
}
