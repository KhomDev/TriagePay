import { setSpending } from "../lib/demo.js";
import { fromError, json, readJson } from "../lib/http.js";

export async function POST(request) {
  try {
    const body = await readJson(request);
    return json(200, await setSpending(body.action, request));
  } catch (error) {
    return fromError(error);
  }
}
