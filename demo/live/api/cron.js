import { timingSafeEqual } from "node:crypto";
import { clearStaleHolds, restoreIfInactive } from "../lib/demo.js";
import { chainReady, loadConfig, loadKeys, signingReady } from "../lib/config.js";
import { fromError, json } from "../lib/http.js";
import { scoreboard } from "../lib/scoreboard.js";

// Daily safety net for a judge who revoked the agent and left. Launch also
// restores before it pays, so the demo does not stay inactive.
export async function GET(request) {
  try {
    // Fail closed: without CRON_SECRET this route signs nothing (red-team
    // L-3). Vercel sends the secret as a bearer token on scheduled runs.
    const secret = String(process.env.CRON_SECRET || "");
    if (!secret) {
      return json(503, { error: { code: "cron_disabled", message: "set CRON_SECRET to enable the daily restore" } });
    }
    const header = request.headers.get("authorization") || "";
    const expected = `Bearer ${secret}`;
    const same = header.length === expected.length
      && timingSafeEqual(Buffer.from(header), Buffer.from(expected));
    if (!same) {
      return json(401, { error: { code: "unauthorized", message: "cron secret required" } });
    }
    const restored = await restoreIfInactive(request);
    const config = loadConfig();
    const keys = loadKeys();
    const cleared = chainReady(config) && signingReady(keys)
      ? await clearStaleHolds(config, keys).catch(() => [])
      : [];
    const board = await scoreboard().catch(() => null);
    return json(200, { ...restored, clearedHolds: cleared, scoreboard: board });
  } catch (error) {
    return fromError(error);
  }
}
