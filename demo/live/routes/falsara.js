// GET  /api/falsara  the wallet's USDC budget and whether Falsara is live
// POST /api/falsara  the mainnet agent buys one Falsara check through its
//                    policy wallet (same-origin JSON only, one per address
//                    every ten minutes; the wallet's daily cap bounds spend)

import { buyFalsaraCheck, falsaraStatus } from "../lib/falsara.js";
import { DemoError, fromError, json, readJson } from "../lib/http.js";
import { clientIp, takeLocalIp } from "../lib/limits.js";

export async function GET() {
  try {
    return json(200, await falsaraStatus());
  } catch (error) {
    return fromError(error);
  }
}

export async function POST(request) {
  try {
    const body = await readJson(request);
    const slot = takeLocalIp(`falsara:${clientIp(request)}`, Date.now(), 1, 10 * 60_000);
    if (!slot.allowed) {
      throw new DemoError("ip_rate_limit", "This address bought a check in the last ten minutes. The receipt above is still valid.", 429, {
        retry_after_seconds: slot.retryAfterSeconds,
      });
    }
    const sample = typeof body.sample === "string" ? body.sample : "admin-drain";
    return json(200, await buyFalsaraCheck({ sample }));
  } catch (error) {
    return fromError(error);
  }
}
