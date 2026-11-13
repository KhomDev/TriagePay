import { restoreIfInactive } from "../lib/demo.js";
import { fromError, json } from "../lib/http.js";
import { scoreboard } from "../lib/scoreboard.js";

// Daily safety net for a judge who revoked the agent and left. Launch also
// restores before it pays, so the demo does not stay inactive.
export async function GET(request) {
  try {
    const secret = process.env.CRON_SECRET;
    if (secret) {
      const header = request.headers.get("authorization") || "";
      if (header !== `Bearer ${secret}`) {
        return json(401, { error: { code: "unauthorized", message: "cron secret required" } });
      }
    }
    const restored = await restoreIfInactive(request);
    const board = await scoreboard().catch(() => null);
    return json(200, { ...restored, scoreboard: board });
  } catch (error) {
    return fromError(error);
  }
}
