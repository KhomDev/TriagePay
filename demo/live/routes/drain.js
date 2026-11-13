import { drainStatus } from "../lib/drain.js";
import { fromError, json, readJson, sse } from "../lib/http.js";
import { runDrainAttempt } from "../lib/live-run.js";
import { formatAtomic } from "../lib/format.js";
import { loadConfig } from "../lib/config.js";
import { honeypotAccount } from "../lib/honeypot.js";
import { honeypotHitsFromChain } from "../lib/txread.js";

function present(status) {
  let drainedDisplay = status.drained;
  try {
    drainedDisplay = formatAtomic(status.drained || "0", loadConfig().policy.decimals);
  } catch {
    drainedDisplay = status.drained;
  }
  return { ...status, drainedDisplay };
}

export async function GET() {
  try {
    const status = await drainStatus();
    if (!status.feed.length) {
      try {
        const config = loadConfig();
        const hits = await honeypotHitsFromChain(config, honeypotAccount(config));
        if (hits.length) {
          status.feed = hits;
          status.attempts = Math.max(status.attempts || 0, hits.length);
          status.refusals = Math.max(status.refusals || 0, hits.filter((row) => row.outcome === "refused").length);
          status.note = "Counted from refused payments on the chain.";
        }
      } catch {
        // The in-memory feed stands.
      }
    }
    return json(200, present(status));
  } catch (error) {
    return fromError(error);
  }
}

export async function POST(request) {
  try {
    const body = await readJson(request);
    const prompt = typeof body.prompt === "string" ? body.prompt : "";
    const wantsStream = (request.headers.get("accept") || "").includes("text/event-stream");
    if (!wantsStream) {
      const result = await runDrainAttempt(request, prompt);
      return json(200, { ...result, drain: present(await drainStatus()) });
    }
    return sse(async (send) => {
      const result = await runDrainAttempt(request, prompt, { onEvent: send });
      send({ type: "result", payload: { ...result, drain: present(await drainStatus()) } });
    });
  } catch (error) {
    return fromError(error);
  }
}
