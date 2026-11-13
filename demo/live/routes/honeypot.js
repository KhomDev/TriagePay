import { loadConfig } from "../lib/config.js";
import { json } from "../lib/http.js";
import { honeypotAccount, listHoneypot } from "../lib/honeypot.js";

export async function GET() {
  const config = loadConfig();
  const list = await listHoneypot();
  return json(200, { ...list, payee: honeypotAccount(config) || null });
}
