// NEAR testnet JSON-RPC and signing. near-api-js is the same 7.3.1 the
// agent demo uses. This module is the only place a secret key is passed
// to a signer.
//
// Static ESM import. createRequire() loads the package's CJS build, and
// that build pulls the ESM-only @noble/curves package, which breaks
// /api/run when Vercel bundles the function.

import * as nearApi from "near-api-js";
import { DemoError, redact } from "./http.js";
import { nsToMs } from "./format.js";

export function near() {
  return nearApi;
}

export function gas(tgas) {
  return BigInt(tgas) * 1_000_000_000_000n;
}

export function parseNear(amount) {
  const api = near();
  const fn = (api.utils && api.utils.format && api.utils.format.parseNearAmount) || api.parseNearAmount;
  if (typeof fn !== "function") throw new DemoError("bad_amount", "near-api-js has no parseNearAmount", 500);
  const parsed = fn(String(amount));
  if (parsed == null) throw new DemoError("bad_amount", "could not parse a NEAR amount", 500);
  return BigInt(parsed);
}

export function randomKeyPair() {
  const pair = near().KeyPair.fromRandom("ed25519");
  return {
    public_key: pair.getPublicKey().toString(),
    secret_key: pair.toString(),
  };
}

export function implicitAccountId(publicKey) {
  const api = near();
  const fn = api.keyToImplicitAddress || (api.utils && api.utils.keyToImplicitAddress);
  if (typeof fn === "function") return fn(publicKey);
  const key = api.utils.PublicKey.fromString(publicKey);
  return Buffer.from(key.data).toString("hex");
}

function unknownAccount(error) {
  const raw = JSON.stringify(error && error.rpc ? error.rpc : error && error.message);
  return /UNKNOWN_ACCOUNT|does not exist|doesn't exist/i.test(raw);
}

export async function rpc(rpcUrl, method, params) {
  let limited = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    const response = await fetch(rpcUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "triagepay-demo", method, params }),
      signal: AbortSignal.timeout(20000),
    });
    if (response.status === 429) {
      limited = new DemoError("rpc_rate_limited", "the NEAR RPC rate-limited the demo", 503);
      await new Promise((resolve) => setTimeout(resolve, 800));
      continue;
    }
    const body = await response.json();
    if (body.error) {
      const error = new Error(body.error.message || "rpc error");
      error.rpc = body.error;
      throw error;
    }
    return body.result;
  }
  throw limited;
}

export async function viewAccount(rpcUrl, accountId) {
  try {
    return await rpc(rpcUrl, "query", {
      request_type: "view_account",
      finality: "optimistic",
      account_id: accountId,
    });
  } catch (error) {
    if (unknownAccount(error)) return null;
    throw error;
  }
}

export async function viewAccessKeys(rpcUrl, accountId) {
  const result = await rpc(rpcUrl, "query", {
    request_type: "view_access_key_list",
    finality: "final",
    account_id: accountId,
  });
  return (result && result.keys) || [];
}

function missingMethod(error) {
  const raw = redact(error && (error.message || String(error)));
  return /MethodNotFound|MethodResolveError|unknown method/i.test(raw);
}

export async function viewCall(rpcUrl, contract, method, args) {
  let result;
  try {
    result = await rpc(rpcUrl, "query", {
      request_type: "call_function",
      finality: "optimistic",
      account_id: contract,
      method_name: method,
      args_base64: Buffer.from(JSON.stringify(args ?? {})).toString("base64"),
    });
  } catch (error) {
    if (missingMethod(error)) {
      throw new DemoError("method_missing", `${method} is not on ${contract}`, 404);
    }
    throw new DemoError("view_failed", `${method} failed on ${contract}`, 502);
  }
  if (!result || result.error) {
    const detail = result && result.error ? String(result.error) : "";
    if (missingMethod(detail) || /MethodNotFound|MethodResolveError/i.test(detail)) {
      throw new DemoError("method_missing", `${method} is not on ${contract}`, 404);
    }
    throw new DemoError("view_failed", `${method} failed on ${contract}`, 502);
  }
  const bytes = Buffer.from(result.result || []);
  if (bytes.length === 0) return null;
  return JSON.parse(bytes.toString("utf8"));
}

export async function assertTestnet(rpcUrl) {
  const status = await rpc(rpcUrl, "status", []);
  if (!status || status.chain_id !== "testnet") {
    throw new DemoError("not_testnet", "the RPC endpoint is not NEAR testnet", 500);
  }
  return status;
}

export function summarize(result) {
  const all = JSON.stringify(result ?? {});
  const failure = Boolean(result && result.status && result.status.Failure);
  const exec = all.match(/ExecutionError":"((?:\\.|[^"\\])*)"/);
  const detail = exec ? exec[1].replace(/\\"/g, "\"").replace(/\\n/g, " ").slice(0, 240) : null;
  const match = (detail || all).match(/ERR_[A-Z0-9_]+/);
  return {
    txHash: (result && result.transaction && result.transaction.hash) || null,
    outcome: failure ? "failure" : "success",
    errorCode: match ? match[0] : null,
    held: !failure && /payment_queued|PaymentQueued/i.test(all),
    rejected: false,
    detail,
  };
}

export async function functionCall(method, args, tgas, deposit = 0n) {
  return near().actions.functionCall(method, args, gas(tgas), deposit);
}

export function deployContract(bytes) {
  return near().actions.deployContract(bytes);
}

export function deleteKey(publicKey) {
  return near().actions.deleteKey(near().PublicKey.fromString(publicKey));
}

export function addPayKey(publicKey, receiverId, allowance) {
  return near().actions.addFunctionCallAccessKey(
    near().PublicKey.fromString(publicKey),
    receiverId,
    ["pay"],
    allowance,
  );
}

export async function signCall({ rpcUrl, signerId, secretKey, receiverId, actions, waitUntil = "EXECUTED" }) {
  const api = near();
  const provider = new api.JsonRpcProvider({ url: rpcUrl });
  const account = new api.Account(signerId, provider, secretKey);
  try {
    const result = await account.signAndSendTransaction({
      receiverId,
      actions,
      waitUntil,
      throwOnFailure: false,
    });
    return summarize(result);
  } catch (error) {
    const raw = redact(error && (error.message || String(error)));
    if (/429|rate limit|timeout|ECONN|fetch failed|EAI_|aborted|Timeout/i.test(raw)) {
      throw new DemoError("rpc_unavailable", "the NEAR RPC did not accept the transaction in time", 503);
    }
    return {
      rejected: true,
      outcome: "rejected",
      errorCode: null,
      txHash: null,
      detail: raw.slice(0, 220),
    };
  }
}

export async function txTimeMs(rpcUrl, hash, signerId) {
  const status = await rpc(rpcUrl, "EXPERIMENTAL_tx_status", {
    tx_hash: hash,
    sender_account_id: signerId,
    wait_until: "NONE",
  });
  const blockHash = status && status.transaction_outcome && status.transaction_outcome.block_hash;
  if (!blockHash) return null;
  const block = await rpc(rpcUrl, "block", { block_id: blockHash });
  return nsToMs(block && block.header && block.header.timestamp);
}
