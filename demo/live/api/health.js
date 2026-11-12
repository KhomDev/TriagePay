import { getHealth } from "../lib/demo.js";
import { fromError, json } from "../lib/http.js";

export async function GET() {
  try {
    return json(200, await getHealth());
  } catch (error) {
    return fromError(error);
  }
}
