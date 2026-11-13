import { drainStatus } from "../lib/drain.js";
import { fromError, json, readJson, sse } from "../lib/http.js";
import { runDrainAttempt } from "../lib/live-run.js";
import { formatAtomic } from "../lib/format.js";
import { loadConfig } from "../lib/config.js";

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
    return json(200, present(await drainStatus()));
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
