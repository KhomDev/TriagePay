import { runLaunch } from "../lib/live-run.js";
import { fromError, json, readJson, sse } from "../lib/http.js";

export async function POST(request) {
  try {
    const body = await readJson(request);
    const scenario = typeof body.scenario === "string" ? body.scenario : "injected-payee";
    const wantsStream = (request.headers.get("accept") || "").includes("text/event-stream");
    if (!wantsStream) return json(200, await runLaunch(request, { scenario }));
    return sse(async (send) => {
      const payload = await runLaunch(request, { scenario, onEvent: send });
      send({ type: "result", payload });
    });
  } catch (error) {
    return fromError(error);
  }
}
