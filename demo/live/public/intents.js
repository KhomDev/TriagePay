// NEAR Intents 1Click client. Endpoints are the ones published at
// https://docs.near-intents.org/integration/distribution-channels/1click-api/quickstart/making-a-request
// on 2026-11-13. This file does not guess a path.
//
// GET  https://1click.chaindefuser.com/v0/tokens
// POST https://1click.chaindefuser.com/v0/quote
// POST https://1click.chaindefuser.com/v0/deposit/submit
// GET  https://1click.chaindefuser.com/v0/status?depositAddress=

export const ONE_CLICK_ORIGIN = "https://1click.chaindefuser.com";
export const TOKENS_PATH = "/v0/tokens";
export const QUOTE_PATH = "/v0/quote";
export const SUBMIT_PATH = "/v0/deposit/submit";
export const STATUS_PATH = "/v0/status";

/** Circle native USDC on NEAR. GET /v0/tokens on 2026-11-13 listed this contract as symbol USDC, 6 decimals. */
export const USDC_CONTRACT = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

export function usdcToken(tokens) {
  return (tokens || []).find((token) => token && token.contractAddress === USDC_CONTRACT) || null;
}

export function toAtomic(amount, decimals) {
  const places = Number(decimals);
  if (!Number.isInteger(places) || places < 0 || places > 36) {
    throw new Error("token decimals are not usable");
  }
  const text = String(amount).trim();
  if (!/^\d+(\.\d+)?$/.test(text)) throw new Error("amount must be a positive decimal");
  const [whole, frac = ""] = text.split(".");
  if (frac.length > places) throw new Error("too many decimal places for this token");
  const digits = (whole + frac.padEnd(places, "0")).replace(/^0+(?=\d)/, "");
  if (digits === "0") throw new Error("amount must be positive");
  return digits;
}

/**
 * Body for POST /v0/quote. `destinationAsset` is the assetId from GET /v0/tokens,
 * not a string I assembled. `dry: true` returns numbers and no deposit address.
 */
export function quoteRequest({
  originAsset,
  destinationAsset,
  amount,
  recipient,
  refundTo,
  dry = true,
  slippageTolerance = 100,
  deadline,
}) {
  if (!originAsset || !destinationAsset || !amount || !recipient || !refundTo) {
    throw new Error("origin, destination, amount, recipient, and refund account are required");
  }
  return {
    dry: Boolean(dry),
    swapType: "EXACT_INPUT",
    slippageTolerance,
    originAsset,
    depositType: "ORIGIN_CHAIN",
    destinationAsset,
    amount: String(amount),
    recipient,
    recipientType: "DESTINATION_CHAIN",
    refundTo,
    refundType: "ORIGIN_CHAIN",
    deadline: deadline || new Date(Date.now() + 30 * 60 * 1000).toISOString(),
  };
}

/**
 * The NEAR call that sends a NEP-141 deposit to the quote's deposit address.
 * Only for an origin token that has a NEAR contract. Other chains are sent
 * from that chain's own wallet; this function does not invent those transfers.
 */
export function nearDepositCall({ contract, depositAddress, amount, memo }) {
  if (!contract || !depositAddress || !amount) {
    throw new Error("contract, deposit address, and amount are required");
  }
  const args = { receiver_id: depositAddress, amount: String(amount) };
  if (memo) args.memo = String(memo);
  return {
    receiverId: contract,
    methodName: "ft_transfer",
    args,
    gas: "30000000000000",
    deposit: "1",
  };
}

async function readJson(response) {
  const text = await response.text();
  let body = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = { message: text.slice(0, 200) };
  }
  if (!response.ok) {
    const message = (body && body.message) || response.status;
    throw new Error("1Click " + response.status + ": " + message);
  }
  return body;
}

export async function getTokens(fetchImpl = fetch, origin = ONE_CLICK_ORIGIN) {
  const response = await fetchImpl(origin + TOKENS_PATH, { headers: { accept: "application/json" } });
  return readJson(response);
}

export async function requestQuote(body, fetchImpl = fetch, origin = ONE_CLICK_ORIGIN) {
  const response = await fetchImpl(origin + QUOTE_PATH, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(body),
  });
  return readJson(response);
}

export async function submitDeposit({ depositAddress, txHash, nearSenderAccount, memo }, fetchImpl = fetch, origin = ONE_CLICK_ORIGIN) {
  const payload = { depositAddress, txHash };
  if (nearSenderAccount) payload.nearSenderAccount = nearSenderAccount;
  if (memo) payload.memo = memo;
  const response = await fetchImpl(origin + SUBMIT_PATH, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(payload),
  });
  return readJson(response);
}

export async function quoteStatus(depositAddress, fetchImpl = fetch, origin = ONE_CLICK_ORIGIN) {
  const url = origin + STATUS_PATH + "?depositAddress=" + encodeURIComponent(depositAddress);
  const response = await fetchImpl(url, { headers: { accept: "application/json" } });
  return readJson(response);
}
