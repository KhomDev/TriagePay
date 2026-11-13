// JSON responses in the API's error shape: {"error":{"code","message"}}.

export function json(status, body, headers = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
      ...headers,
    },
  });
}

export function fail(status, code, message, extra = {}, headers = {}) {
  return json(status, { error: { code, message }, ...extra }, headers);
}

export function redact(value) {
  return String(value == null ? "" : value)
    .replace(/ed25519:[1-9A-HJ-NP-Za-km-z]+/g, "ed25519:<redacted>")
    .replace(/\d{6,}:[A-Za-z0-9_-]{20,}/g, "<redacted-telegram>")
    .replace(/Bearer\s+\S+/gi, "Bearer <redacted>");
}

export class DemoError extends Error {
  constructor(code, message, status = 400, extra = {}) {
    super(message);
    this.code = code;
    this.status = status;
    this.extra = extra;
  }
}

export function fromError(error) {
  if (error instanceof DemoError) {
    const headers = error.code === "cooldown" || error.code === "daily_attempt_cap" || error.code === "ip_rate_limit"
      ? { "retry-after": String(error.extra.retry_after_seconds || 1) }
      : {};
    return fail(error.status, error.code, error.message, error.extra, headers);
  }
  const message = redact(error && error.message ? error.message : error).slice(0, 240);
  return fail(500, "demo_failed", message || "the demo request failed");
}

export function sse(work) {
  const encoder = new TextEncoder();
  const stream = new ReadableStream({
    async start(controller) {
      const send = (event) => {
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`));
      };
      try {
        await work(send);
      } catch (error) {
        const message = error instanceof DemoError
          ? error.message
          : redact(error && error.message ? error.message : error);
        send({ type: "error", message: String(message || "the demo request failed").slice(0, 240) });
      } finally {
        controller.close();
      }
    },
  });
  return new Response(stream, {
    status: 200,
    headers: {
      "content-type": "text/event-stream; charset=utf-8",
      "cache-control": "no-store",
      "x-accel-buffering": "no",
    },
  });
}

function hostsOf(request) {
  const hosts = new Set();
  try {
    hosts.add(new URL(request.url).host.toLowerCase());
  } catch {
    // A relative URL has no host to add.
  }
  for (const name of ["host", "x-forwarded-host"]) {
    const value = request.headers.get(name);
    if (value) hosts.add(value.split(",")[0].trim().toLowerCase());
  }
  return hosts;
}

const CROSS_ORIGIN = "This action only runs from the demo page itself.";

/**
 * Throws 403 unless the request comes from this site's own page (or from
 * a non-browser client, which sends no Origin). A third-party page can
 * fire a "simple" cross-origin POST without a preflight, but it cannot
 * forge Origin or Sec-Fetch-Site (red-team RT-11).
 */
export function assertSameOrigin(request) {
  const site = (request.headers.get("sec-fetch-site") || "").toLowerCase();
  if (site && site !== "same-origin" && site !== "none") {
    throw new DemoError("cross_origin", CROSS_ORIGIN, 403);
  }
  const origin = request.headers.get("origin");
  if (origin == null) return;
  let host = null;
  try {
    host = origin === "null" ? null : new URL(origin).host.toLowerCase();
  } catch {
    host = null;
  }
  if (!host || !hostsOf(request).has(host)) throw new DemoError("cross_origin", CROSS_ORIGIN, 403);
}

async function readCapped(request, max) {
  const declared = Number(request.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > max) {
    throw new DemoError("body_too_large", "request body is too large", 413);
  }
  if (!request.body || typeof request.body.getReader !== "function") {
    const text = await request.text();
    if (text.length > max) throw new DemoError("body_too_large", "request body is too large", 413);
    return text;
  }
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > max) {
      await reader.cancel().catch(() => {});
      throw new DemoError("body_too_large", "request body is too large", 413);
    }
    chunks.push(value);
  }
  return Buffer.concat(chunks.map((chunk) => Buffer.from(chunk))).toString("utf8");
}

/**
 * Read a small JSON object body. Every POST that can sign goes through
 * here, so this is also where the request must be same-origin and typed
 * application/json (a cross-site form cannot send that type without a
 * preflight). `options.crossOrigin` is for webhooks that are not called
 * by a browser (Telegram), which authenticate on their own.
 */
export async function readJson(request, max = 4096, options = {}) {
  if (!options.crossOrigin) assertSameOrigin(request);
  const type = (request.headers.get("content-type") || "").toLowerCase();
  const text = await readCapped(request, max);
  if (!text.trim()) return {};
  if (!type.startsWith("application/json")) {
    throw new DemoError("bad_content_type", "send the body as application/json", 415);
  }
  try {
    const value = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new DemoError("bad_json", "request body must be a JSON object", 400);
    }
    return value;
  } catch (error) {
    if (error instanceof DemoError) throw error;
    throw new DemoError("bad_json", "request body is not JSON", 400);
  }
}
