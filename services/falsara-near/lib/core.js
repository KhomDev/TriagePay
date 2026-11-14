// Call the Falsara core: the pinned viability-gate and realism-gate behind a
// shared internal token. The core is Falsara's existing product code; only
// this gateway, which verifies NEAR payments, may call it.

export async function runCore(config, route, body, fetchImpl = fetch) {
  let response;
  try {
    response = await fetchImpl(config.coreUrl + route.corePath, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-ail-internal-token": config.coreToken,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(45_000),
    });
  } catch {
    return { status: 503, body: { error: "CORE_UNAVAILABLE" } };
  }
  let parsed = null;
  try {
    parsed = await response.json();
  } catch {
    parsed = null;
  }
  if (response.status === 200 && parsed && typeof parsed.verdict === "string") {
    return { status: 200, body: parsed };
  }
  if (response.status === 400 && parsed) return { status: 400, body: parsed };
  return { status: 503, body: { error: "CORE_UNAVAILABLE" } };
}

/** Names whose reported pin differs from the expected one (missing counts). */
export function pinDrift(expected, reported) {
  const got = reported && typeof reported === "object" ? reported : {};
  return Object.keys(expected).filter((name) => String(got[name]) !== expected[name]);
}

export async function coreHealth(config, fetchImpl = fetch) {
  const expected = config.expectedPins || {};
  if (!config.coreUrl) return { ok: false, pinsMatch: false, expectedPins: expected };
  try {
    const response = await fetchImpl(config.coreUrl + "/health", { signal: AbortSignal.timeout(8_000) });
    const body = await response.json();
    const pins = body && body.pins;
    const drift = pinDrift(expected, pins);
    return {
      ok: Boolean(response.ok && body && body.status === "ok"),
      pins,
      pinsMatch: drift.length === 0,
      drift,
      expectedPins: expected,
      gates: body && body.gates,
    };
  } catch {
    return { ok: false, pinsMatch: false, expectedPins: expected };
  }
}
