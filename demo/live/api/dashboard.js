import { getDashboard } from "../lib/demo.js";
import { fromError, json } from "../lib/http.js";

export async function GET() {
  try {
    return json(200, await getDashboard());
  } catch (error) {
    return fromError(error);
  }
}
