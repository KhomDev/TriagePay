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

export async function coreHealth(config, fetchImpl = fetch) {
  if (!config.coreUrl) return { ok: false };
  try {
    const response = await fetchImpl(config.coreUrl + "/health", { signal: AbortSignal.timeout(8_000) });
    const body = await response.json();
    return { ok: response.ok && body && body.status === "ok", pins: body && body.pins, gates: body && body.gates };
  } catch {
    return { ok: false };
  }
}
