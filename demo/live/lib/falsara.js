// The agent buys a Falsara check on NEAR mainnet, through its policy wallet.
//
// 1. POST the finding to Falsara; a decidable finding answers 402 with a USDC
//    requirement on near:mainnet.
// 2. Check the requirement before paying: mainnet, native USDC, a payee on
//    the wallet's allowlist, an amount inside the wallet's per-call cap.
// 3. The agent's pay-only key calls `pay` on the wallet with the memo. The
//    contract checks the payee and both caps, then sends the USDC.
// 4. Retry with PAYMENT-SIGNATURE until Falsara has read the final receipt.
//
// The agent never holds a key that can move tokens itself; the wallet's
// daily USDC cap is what bounds what this public button can spend.

import { DemoError } from "./http.js";
import { MAINNET, mainnetAgentKey } from "./mainnet.js";
import { functionCall, rpc, signCall, viewCall } from "./near.js";

export const USDC_MAINNET = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
const ACCOUNT = /^(?=.{2,64}$)[a-z0-9]+([-_.][a-z0-9]+)*$/;

export function falsaraUrl(env = process.env) {
  return String(env.FALSARA_URL || "https://api.falsara.online").replace(/\/$/, "");
}

/** Sample findings the button can buy a check for. */
export const SAMPLES = {
  "admin-drain": {
    label: "An admin can drain the vault",
    finding: {
      schema_version: "1.0.0",
      title: "Owner can withdraw all user deposits",
      severity: "high",
      protocol: "ExampleVault",
      contract: "src/Vault.sol",
      impact: "The owner calls emergencyWithdraw and moves every user deposit to an address the owner picks.",
      preconditions: "The caller holds the owner role, which only the deployer has.",
    },
  },
};

function decode(raw) {
  try {
    return JSON.parse(Buffer.from(String(raw || ""), "base64").toString("utf8"));
  } catch {
    return null;
  }
}

/** The checks the agent makes before it signs anything. */
export function acceptableRequirement(accepted, { payees, perCallMax }) {
  if (!accepted || typeof accepted !== "object") return "no requirement";
  if (accepted.scheme !== "exact" || accepted.network !== "near:mainnet") return "not an exact NEAR mainnet payment";
  if (accepted.asset !== USDC_MAINNET) return "not native USDC";
  if (!ACCOUNT.test(String(accepted.payTo || "")) || !(payees || []).includes(accepted.payTo)) return "the payee is not on the wallet allowlist";
  if (!/^\d+$/.test(String(accepted.amount || "")) || BigInt(accepted.amount) <= 0n) return "no amount";
  if (perCallMax != null && BigInt(accepted.amount) > BigInt(perCallMax)) return "above the wallet's per-call cap";
  const memo = accepted.extra && accepted.extra.memo;
  if (!/^fal1_[0-9a-z_]{20,80}$/.test(String(memo || ""))) return "no Falsara memo";
  return null;
}

async function postFinding(url, finding, signature, fetchImpl) {
  const headers = { "content-type": "application/json" };
  if (signature) headers["payment-signature"] = signature;
  const response = await fetchImpl(`${url}/v1/check-finding`, {
    method: "POST",
    headers,
    body: JSON.stringify(finding),
    signal: AbortSignal.timeout(50_000),
  });
  let body = null;
  try {
    body = await response.json();
  } catch {
    body = null;
  }
  return { status: response.status, body, required: decode(response.headers.get("payment-required")) };
}

function usdc(atomic) {
  const value = BigInt(atomic || "0");
  const whole = value / 1_000_000n;
  const frac = String(value % 1_000_000n).padStart(6, "0").replace(/0+$/, "");
  return `${whole}${frac ? "." + frac : ""} USDC`;
}

export async function buyFalsaraCheck({ sample = "admin-drain", env = process.env, fetchImpl = fetch, sleep = (ms) => new Promise((r) => setTimeout(r, ms)), chain = {} } = {}) {
  const view = chain.viewCall || viewCall;
  const sign = chain.signCall || signCall;
  const chosen = SAMPLES[sample];
  if (!chosen) throw new DemoError("unknown_sample", "Pick one of the sample findings.", 400);
  const secretKey = mainnetAgentKey(env);
  if (!secretKey) throw new DemoError("mainnet_off", "Mainnet signing is off on this deployment.", 409);
  const url = falsaraUrl(env);
  const steps = [];

  const quote = await postFinding(url, chosen.finding, null, fetchImpl);
  if (quote.status === 422) {
    return { steps: [{ name: "Falsara", result: "Cannot decide this finding yet. Nothing was charged." }], verdict: "INCOMPLETE", charged: false, falsara: quote.body };
  }
  if (quote.status !== 402 || !quote.required || !Array.isArray(quote.required.accepts)) {
    throw new DemoError("falsara_unavailable", `Falsara answered ${quote.status}.`, 502);
  }
  const accepted = quote.required.accepts[0];
  steps.push({ name: "Falsara asks for payment", result: `${usdc(accepted.amount)} on NEAR mainnet to ${accepted.payTo}` });

  const agent = await view(MAINNET.rpcUrl, MAINNET.accounts.wallet, "get_agent", { agent_id: MAINNET.accounts.agent });
  const budget = agent && Array.isArray(agent.budgets) ? agent.budgets.find((row) => row.token === USDC_MAINNET) : null;
  const problem = acceptableRequirement(accepted, { payees: agent && agent.payees, perCallMax: budget && budget.per_call_max });
  if (problem) throw new DemoError("requirement_refused", `The agent did not pay: ${problem}.`, 409);
  if (budget && BigInt(budget.remaining) < BigInt(accepted.amount)) {
    throw new DemoError("daily_budget_spent", `The wallet's USDC budget for today is spent (${usdc(budget.remaining)} left). The contract would refuse; try again after 00:00 UTC.`, 429, { retry_after_seconds: 3600 });
  }

  const action = await functionCall("pay", {
    token: USDC_MAINNET,
    receiver_id: accepted.payTo,
    amount: String(accepted.amount),
    memo: accepted.extra.memo,
  }, 50, 0n);
  const signed = await sign({
    rpcUrl: MAINNET.rpcUrl,
    signerId: MAINNET.accounts.agent,
    secretKey,
    receiverId: MAINNET.accounts.wallet,
    actions: [action],
  });
  const payTx = {
    txHash: signed.txHash,
    explorerUrl: signed.txHash ? MAINNET.explorerTxBase + signed.txHash : null,
    outcome: signed.outcome,
    errorCode: signed.errorCode || null,
    held: Boolean(signed.held),
  };
  if (signed.rejected) {
    steps.push({ name: "NEAR", result: `The node rejected the transaction before it ran, so nothing moved.${signed.detail ? ` ${signed.detail}` : ""}` });
    return { steps, verdict: null, charged: false, pay: payTx };
  }
  if (signed.outcome !== "success" || signed.errorCode) {
    steps.push({ name: "The wallet", result: `Refused the payment${signed.errorCode ? ` (${signed.errorCode})` : ""}. Nothing moved.` });
    return { steps, verdict: null, charged: false, pay: payTx };
  }
  if (signed.held) {
    // pay() returned success but queued the payment for the owner: no USDC moved.
    steps.push({ name: "The wallet", result: `Held the payment of ${usdc(accepted.amount)} for the owner's review. No USDC moved, so Falsara was not paid and no check ran.` });
    return { steps, verdict: null, charged: false, pay: payTx };
  }
  steps.push({ name: "The wallet", result: `Checked the payee and the caps, then paid ${usdc(accepted.amount)}.` });

  const signature = Buffer.from(JSON.stringify({
    x402Version: 2,
    resource: quote.required.resource,
    accepted,
    payload: { network: "near:mainnet", intent_id: accepted.extra.memo, tx_hash: signed.txHash, signer_id: MAINNET.accounts.agent },
  })).toString("base64");
  let result = null;
  for (let attempt = 0; attempt < 10; attempt += 1) {
    result = await postFinding(url, chosen.finding, signature, fetchImpl);
    if (result.status === 200) break;
    const reason = result.body && result.body.error;
    if (reason !== "settlement_pending" && result.status !== 503) break;
    await sleep(2500);
  }
  if (!result || result.status !== 200) {
    steps.push({ name: "Falsara", result: `Did not accept the proof yet (${result && result.body && result.body.error}). The payment is on chain; the same proof can be retried.` });
    return { steps, verdict: null, charged: true, pay: payTx };
  }
  const verdict = result.body;
  steps.push({ name: "Falsara", result: `Verified the USDC transfer on chain and ran the gates: ${verdict.verdict}.` });
  return {
    steps,
    sample: { id: sample, label: chosen.label, finding: chosen.finding },
    verdict: verdict.verdict,
    hits: (verdict.hits || []).slice(0, 4).map((hit) => ({
      rule: hit.rule || hit.id || null,
      category: hit.category || null,
      quote: String(hit.explanation || hit.quote || hit.message || "").slice(0, 280),
    })),
    missingEvidence: verdict.missing_evidence || [],
    charged: true,
    amount: usdc(accepted.amount),
    pay: payTx,
    payment: verdict.payment || null,
  };
}

/** Live view for the page: the wallet's USDC budget and Falsara's status. */
export async function falsaraStatus(env = process.env, fetchImpl = fetch) {
  const url = falsaraUrl(env);
  let health = null;
  try {
    const response = await fetchImpl(`${url}/health`, { signal: AbortSignal.timeout(6000) });
    health = await response.json();
  } catch {
    health = null;
  }
  let budget = null;
  try {
    const agent = await viewCall(MAINNET.rpcUrl, MAINNET.accounts.wallet, "get_agent", { agent_id: MAINNET.accounts.agent });
    budget = agent && Array.isArray(agent.budgets) ? agent.budgets.find((row) => row.token === USDC_MAINNET) || null : null;
  } catch {
    budget = null;
  }
  return {
    url,
    live: Boolean(health && health.ok),
    signing: Boolean(mainnetAgentKey(env)),
    budget: budget ? {
      perCall: usdc(budget.per_call_max),
      daily: usdc(budget.daily_max),
      remaining: usdc(budget.remaining),
      checksLeft: Number(BigInt(budget.remaining) / (BigInt(budget.per_call_max) || 1n)),
    } : null,
  };
}

export { rpc };
