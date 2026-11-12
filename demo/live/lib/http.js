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
  return String(value == null ? "" : value).replace(/ed25519:[1-9A-HJ-NP-Za-km-z]+/g, "ed25519:<redacted>");
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
    const headers = error.code === "cooldown" || error.code === "daily_attempt_cap"
      ? { "retry-after": String(error.extra.retry_after_seconds || 1) }
      : {};
    return fail(error.status, error.code, error.message, error.extra, headers);
  }
  const message = redact(error && error.message ? error.message : error).slice(0, 240);
  return fail(500, "demo_failed", message || "the demo request failed");
}

export async function readJson(request) {
  const text = await request.text();
  if (text.length > 4096) throw new DemoError("body_too_large", "request body is too large", 413);
  if (!text.trim()) return {};
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
