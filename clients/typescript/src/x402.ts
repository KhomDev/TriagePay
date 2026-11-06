// x402 v2 client helpers for the near-tx-proof method (no NEAR dependency).
//
// Headers carry base64 JSON: PAYMENT-REQUIRED (402), PAYMENT-SIGNATURE
// (retry), PAYMENT-RESPONSE (settlement). See docs/protocols/x402-near.md.

export const X402_VERSION = 2;
export const SCHEME = "exact";
export const TRANSFER_METHOD = "near-tx-proof";
export const PAYMENT_FLOW = "upfront";

export type Requirement = {
  scheme: string;
  network: string;
  amount: string;
  asset: string;
  payTo: string;
  maxTimeoutSeconds: number;
  extra: { assetTransferMethod?: string; paymentFlow?: string; memo?: string; [key: string]: unknown };
};

export type Proof = { tx_hash: string; signer_id: string };

export type Payer = (requirement: Requirement) => Promise<Proof>;

export class X402Error extends Error {
  status?: number;
  reason?: string;
  paymentResponse?: Record<string, unknown>;
  body?: unknown;
  constructor(message: string, details: { status?: number; reason?: string; paymentResponse?: Record<string, unknown>; body?: unknown } = {}) {
    super(message);
    this.name = "X402Error";
    this.status = details.status;
    this.reason = details.reason;
    this.paymentResponse = details.paymentResponse;
    this.body = details.body;
  }
}

export function encodeHeader(value: unknown): string {
  return Buffer.from(JSON.stringify(value), "utf8").toString("base64");
}

export function decodeHeader(raw: string): any {
  const text = raw.trim().replace(/-/g, "+").replace(/_/g, "/");
  try {
    return JSON.parse(Buffer.from(text, "base64").toString("utf8"));
  } catch {
    throw new X402Error("header is not base64 JSON");
  }
}

/** Pick the near-tx-proof upfront entry; unknown flows are skipped. */
export function selectRequirement(paymentRequired: any, network?: string): Requirement {
  if (!paymentRequired || paymentRequired.x402Version !== X402_VERSION) {
    throw new X402Error("the server did not answer with an x402 v2 PaymentRequired");
  }
  for (const entry of paymentRequired.accepts ?? []) {
    const extra = entry.extra ?? {};
    if (entry.scheme !== SCHEME) continue;
    if (extra.assetTransferMethod !== TRANSFER_METHOD) continue;
    if ((extra.paymentFlow ?? PAYMENT_FLOW) !== PAYMENT_FLOW) continue;
    if (network !== undefined && entry.network !== network) continue;
    if (!extra.memo) continue;
    return entry as Requirement;
  }
  throw new X402Error("no near-tx-proof requirement this client can pay was offered");
}

export function buildPaymentPayload(requirement: Requirement, proof: Proof, resource?: unknown): Record<string, unknown> {
  const txHash = String(proof?.tx_hash ?? "").trim();
  const signerId = String(proof?.signer_id ?? "").trim();
  if (!txHash || !signerId) throw new X402Error("the payer must return non-empty tx_hash and signer_id");
  const payload: Record<string, unknown> = {
    x402Version: X402_VERSION,
    accepted: requirement,
    payload: { network: requirement.network, intent_id: requirement.extra.memo, tx_hash: txHash, signer_id: signerId },
  };
  if (resource) payload.resource = resource;
  return payload;
}

export type PaidScan = {
  body: any;
  paymentResponse: Record<string, unknown>;
  requirement: Requirement;
  proof: Proof;
  attempts: number;
};

export type PayAndScanOptions = {
  path?: string;
  network?: string;
  maxPendingRetries?: number;
  pendingDelayMs?: number;
  sleep?: (ms: number) => Promise<void>;
  fetchImpl?: typeof fetch;
};

const defaultSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

async function post(fetchImpl: typeof fetch, url: string, body: unknown, headers: Record<string, string> = {}) {
  const response = await fetchImpl(url, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  let parsed: any = {};
  try {
    parsed = text ? JSON.parse(text) : {};
  } catch {
    parsed = { raw: text };
  }
  return { status: response.status, headers: response.headers, body: parsed };
}

/** Request, pay, retry: one paid call of an x402 near-tx-proof resource. */
export async function payAndScan(baseUrl: string, envelope: unknown, pay: Payer, options: PayAndScanOptions = {}): Promise<PaidScan> {
  const fetchImpl = options.fetchImpl ?? fetch;
  const sleep = options.sleep ?? defaultSleep;
  const url = baseUrl.replace(/\/+$/, "") + (options.path ?? "/v2/scan");
  const first = await post(fetchImpl, url, envelope);
  if (first.status !== 402) throw new X402Error(`expected 402 Payment Required, got ${first.status}`, { status: first.status, body: first.body });
  const rawRequired = first.headers.get("payment-required");
  if (!rawRequired) throw new X402Error("402 without a PAYMENT-REQUIRED header", { status: 402, body: first.body });
  const paymentRequired = decodeHeader(rawRequired);
  const requirement = selectRequirement(paymentRequired, options.network);
  const proof = await pay(requirement);
  const signature = encodeHeader(buildPaymentPayload(requirement, proof, paymentRequired.resource));
  const maxRetries = options.maxPendingRetries ?? 40;
  for (let attempts = 1; ; attempts++) {
    const reply = await post(fetchImpl, url, envelope, { "PAYMENT-SIGNATURE": signature });
    const rawResponse = reply.headers.get("payment-response");
    const settlement = rawResponse ? decodeHeader(rawResponse) : {};
    if (reply.status === 200) {
      if (!settlement.success) throw new X402Error("200 without a successful PAYMENT-RESPONSE", { status: 200, body: reply.body });
      return { body: reply.body, paymentResponse: settlement, requirement, proof, attempts };
    }
    const reason = settlement.errorReason ?? reply.body?.error?.code;
    if (reply.status === 402 && reason === "settlement_pending" && attempts <= maxRetries) {
      const retryAfter = Number(reply.headers.get("retry-after"));
      await sleep(options.pendingDelayMs ?? (Number.isFinite(retryAfter) && retryAfter >= 0 ? retryAfter * 1000 : 2000));
      continue;
    }
    throw new X402Error(`payment failed (${reply.status} ${reason}): ${reply.body?.error?.message ?? ""}`, {
      status: reply.status,
      reason,
      paymentResponse: settlement,
      body: reply.body,
    });
  }
}
