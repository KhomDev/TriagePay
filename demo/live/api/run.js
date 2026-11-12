import { runScenario } from "../lib/demo.js";
import { fromError, json, readJson } from "../lib/http.js";

export async function POST(request) {
  try {
    const body = await readJson(request);
    const id = typeof body.scenario === "string" ? body.scenario : body.attack;
    return json(200, await runScenario(id, request));
  } catch (error) {
    return fromError(error);
  }
}
