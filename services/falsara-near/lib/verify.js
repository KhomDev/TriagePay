// Payment verification from the chain. A client hint (transaction hash and
// signer) only locates the transaction; every fact is read from the
// finalized receipts, following the rules TriagePay's Rust verifier uses
// (crates/near-adapter/src/verify.rs):
//
// - the transaction is FINAL;
// - the funding receipt was executed by the asset contract itself and
//   succeeded, so no other contract's log can count;
// - that receipt's own NEP-141 ft_transfer event pays the payee, with the
//   server-issued memo, for exactly the price;
// - the block it executed in is not later than the requirement's expiry.

const EVENT_PREFIX = "EVENT_JSON:";

export function nep141Transfers(logs) {
  const out = [];
  for (const line of logs || []) {
    if (typeof line !== "string" || !line.startsWith(EVENT_PREFIX)) continue;
    let event;
    try {
      event = JSON.parse(line.slice(EVENT_PREFIX.length).trim());
    } catch {
      continue;
    }
    if (!event || event.standard !== "nep141" || event.event !== "ft_transfer" || !Array.isArray(event.data)) continue;
    for (const item of event.data) {
      if (item && typeof item === "object") out.push(item);
    }
  }
  return out;
}

function succeeded(status) {
  return Boolean(status) && (Object.prototype.hasOwnProperty.call(status, "SuccessValue")
    || Object.prototype.hasOwnProperty.call(status, "SuccessReceiptId"));
}

/**
 * Match one transaction against the terms and the memo, without the block
 * time. Returns { status, ... } where status is one of
 * "confirmed" | "pending" | "failed" | "mismatch".
 */
export function matchTransaction(result, terms, memo) {
  if (!result || typeof result !== "object") return { status: "pending" };
  if (result.final_execution_status && result.final_execution_status !== "FINAL") return { status: "pending" };
  if (result.status && result.status.Failure) return { status: "failed", reason: "the transaction failed" };
  const receipts = Array.isArray(result.receipts_outcome) ? result.receipts_outcome : [];
  let wrongAmount = null;
  for (const receipt of receipts) {
    const outcome = receipt && receipt.outcome;
    if (!outcome || outcome.executor_id !== terms.asset || !succeeded(outcome.status)) continue;
    for (const transfer of nep141Transfers(outcome.logs)) {
      if (transfer.new_owner_id !== terms.payTo || transfer.memo !== memo) continue;
      if (String(transfer.amount) !== terms.amount) {
        wrongAmount = String(transfer.amount);
        continue;
      }
      return {
        status: "confirmed",
        receiptId: receipt.id,
        blockHash: receipt.block_hash,
        payer: String(transfer.old_owner_id || ""),
        amount: String(transfer.amount),
      };
    }
  }
  if (wrongAmount) return { status: "mismatch", code: "amount_mismatch", reason: `the transfer moved ${wrongAmount}, the price is ${terms.amount}` };
  return { status: "mismatch", code: "invalid_payload", reason: "no transfer from the asset contract pays this payee with this memo" };
}

async function rpc(url, method, params, fetchImpl) {
  const response = await fetchImpl(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "falsara", method, params }),
    signal: AbortSignal.timeout(12_000),
  });
  if (!response.ok) {
    const error = new Error(`${method} HTTP ${response.status}`);
    error.transient = true;
    throw error;
  }
  const body = await response.json();
  if (body.error) {
    const error = new Error(`${method}: ${String(body.error.message || "error")}`);
    error.rpc = body.error;
    throw error;
  }
  return body.result;
}

function unknownTransaction(error) {
  const cause = error && error.rpc && error.rpc.cause && error.rpc.cause.name;
  return cause === "UNKNOWN_TRANSACTION" || /doesn't exist|UNKNOWN_TRANSACTION/.test(String(error && error.message));
}

/** Read the transaction, falling back to the archival node for old ones. */
async function readTransaction(endpoints, txHash, signerId, fetchImpl) {
  const params = { tx_hash: txHash, sender_account_id: signerId, wait_until: "FINAL" };
  let lastError = null;
  for (const url of endpoints) {
    try {
      return await rpc(url, "EXPERIMENTAL_tx_status", params, fetchImpl);
    } catch (error) {
      lastError = error;
      if (!unknownTransaction(error) && !error.transient) throw error;
    }
  }
  if (lastError && unknownTransaction(lastError)) return null;
  throw lastError || new Error("no RPC endpoint answered");
}

async function blockTimeUnix(endpoints, blockHash, fetchImpl) {
  for (const url of endpoints) {
    try {
      const block = await rpc(url, "block", { block_id: blockHash }, fetchImpl);
      const ns = block && block.header && (block.header.timestamp_nanosec || block.header.timestamp);
      if (ns != null) return Number(BigInt(String(ns)) / 1_000_000_000n);
    } catch {
      // try the next endpoint
    }
  }
  return null;
}

/**
 * Verify a payment. Returns { status, ... }; status "confirmed" means the
 * memo's transfer is final, exact, from the asset contract, and landed
 * before `expiresAt`.
 */
export async function verifyPayment({ endpoints, terms, memo, txHash, signerId, expiresAt, fetchImpl = fetch }) {
  let result;
  try {
    result = await readTransaction(endpoints, txHash, signerId, fetchImpl);
  } catch {
    return { status: "unavailable" };
  }
  if (!result) return { status: "pending" };
  const matched = matchTransaction(result, terms, memo);
  if (matched.status !== "confirmed") return matched;
  const landed = await blockTimeUnix(endpoints, matched.blockHash, fetchImpl);
  if (landed == null) return { status: "unavailable" };
  if (landed > expiresAt) return { status: "expired", landed };
  return { ...matched, landed };
}
