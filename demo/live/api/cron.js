import { restoreIfInactive } from "../lib/demo.js";
import { fromError, json } from "../lib/http.js";

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
    return json(200, await restoreIfInactive(request));
  } catch (error) {
    return fromError(error);
  }
}
