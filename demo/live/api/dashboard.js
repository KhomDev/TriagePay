import { getDashboard } from "../lib/demo.js";
import { fromError, json } from "../lib/http.js";

let lastGood = null;

async function load() {
  let lastError = null;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      return await getDashboard();
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}

export async function GET() {
  try {
    const body = await load();
    lastGood = body;
    return json(200, body);
  } catch (error) {
    if (lastGood) return json(200, { ...lastGood, stale: true });
    return fromError(error);
  }
}
