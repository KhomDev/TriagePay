// Minimal x402 v2 client for the near-tx-proof method. The payer is
// injected, so this file does not import a NEAR library.

export const X402_VERSION = 2;

export function encodeHeader(value) {
  return Buffer.from(JSON.stringify(value), "utf8").toString("base64");
}

export function decodeHeader(raw) {
  const text = String(raw || "").trim().replace(/-/g, "+").replace(/_/g, "/");
  return JSON.parse(Buffer.from(text, "base64").toString("utf8"));
}

export function selectRequirement(paymentRequired, network) {
  if (!paymentRequired || paymentRequired.x402Version !== X402_VERSION) {
    throw new Error("the server did not answer with an x402 v2 payment requirement");
  }
  for (const entry of paymentRequired.accepts || []) {
    const extra = entry.extra || {};
    if (entry.scheme !== "exact") continue;
    if (extra.assetTransferMethod !== "near-tx-proof") continue;
    if ((extra.paymentFlow || "upfront") !== "upfront") continue;
    if (network !== undefined && entry.network !== network) continue;
    if (!extra.memo) continue;
    return entry;
  }
  throw new Error("no near-tx-proof requirement was offered");
}

export function buildPaymentPayload(requirement, proof) {
  return {
    x402Version: X402_VERSION,
    accepted: requirement,
    payload: {
      network: requirement.network,
      intent_id: requirement.extra.memo,
      tx_hash: proof.tx_hash,
      signer_id: proof.signer_id,
    },
  };
}

/**
 * POST `body`, and if the server answers 402, pay with `payer` and retry.
 * `payer` is usually `createPolicyPayer(...)`.
 */
export async function payPerCall({ fetchImpl = fetch, url, body, payer, network, init = {} }) {
  const payload = JSON.stringify(body ?? {});
  const headers = { "content-type": "application/json", ...(init.headers || {}) };
  const first = await fetchImpl(url, { ...init, method: "POST", headers, body: payload });
  if (first.status !== 402) return { response: first, paid: false };
  const header = first.headers.get("payment-required") || first.headers.get("PAYMENT-REQUIRED");
  const requirement = selectRequirement(decodeHeader(header), network);
  const proof = await payer(requirement);
  const secondHeaders = {
    ...headers,
    "payment-signature": encodeHeader(buildPaymentPayload(requirement, proof)),
  };
  const second = await fetchImpl(url, { ...init, method: "POST", headers: secondHeaders, body: payload });
  return { response: second, paid: true, proof, requirement };
}
