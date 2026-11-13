import { runLaunch } from "../lib/live-run.js";
import { runMainnetLaunch } from "../lib/mainnet.js";
import { fromError, json, readJson, sse } from "../lib/http.js";

export async function POST(request) {
  try {
    const body = await readJson(request);
    const scenario = typeof body.scenario === "string" ? body.scenario : "injected-payee";
    const mainnet = body.network === "mainnet";
    const wantsStream = (request.headers.get("accept") || "").includes("text/event-stream");
    if (mainnet) {
      if (!wantsStream) return json(200, await runMainnetLaunch(request));
      return sse(async (send) => {
        send({ type: "status", text: "Signing on NEAR mainnet." });
        const payload = await runMainnetLaunch(request);
        send({ type: "result", payload });
      });
    }
    if (!wantsStream) return json(200, await runLaunch(request, { scenario }));
    return sse(async (send) => {
      const payload = await runLaunch(request, { scenario, onEvent: send });
      send({ type: "result", payload });
    });
  } catch (error) {
    return fromError(error);
  }
}
