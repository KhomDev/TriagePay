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
    try {
      const config = loadConfig();
      const hits = await honeypotHitsFromChain(config, honeypotAccount(config));
      const byHash = new Map();
      for (const row of [...hits, ...(status.feed || [])]) {
        if (!row) continue;
        const key = row.txHash || `local-${row.atMs || ""}-${row.prompt || ""}`;
        const prev = byHash.get(key) || {};
        byHash.set(key, { ...prev, ...row });
      }
      const feed = [...byHash.values()].sort((a, b) => (b.atMs || 0) - (a.atMs || 0));
      if (feed.length) {
        status.feed = feed;
        status.attempts = feed.length;
        status.refusals = feed.filter((row) => row.outcome === "refused" || row.errorCode).length;
        status.note = "Counted from the agent's payments on the chain.";
      }
    } catch {
      // Keep the feed this process already has.
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
