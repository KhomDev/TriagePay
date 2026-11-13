import { loadConfig } from "../lib/config.js";
import { json } from "../lib/http.js";
import { loadReplay, partialReplay } from "../lib/replay.js";

export async function GET(request) {
  const tx = new URL(request.url).searchParams.get("tx") || "";
  const found = await loadReplay(tx);
  if (found) return json(200, found);
  const config = loadConfig();
  const partial = partialReplay(tx, config.explorerTxBase);
  if (!partial) return json(400, { error: { code: "bad_tx", message: "That transaction hash is not usable." } });
  return json(200, partial);
}
