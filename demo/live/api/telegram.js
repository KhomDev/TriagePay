import { decideReview } from "../lib/demo.js";
import { DemoError, fromError, json, readJson } from "../lib/http.js";
import { answerCallback, parseCallback, telegramConfig, verifyTelegramHeader } from "../lib/telegram.js";

export async function POST(request) {
  const cfg = telegramConfig();
  const header = verifyTelegramHeader(request, cfg);
  if (!header.ok) return json(header.status, { error: { code: header.code, message: header.message } });
  try {
    const update = await readJson(request, 65536, { crossOrigin: true });
    const parsed = parseCallback(update, cfg.chatId);
    if (parsed.ignore) return json(200, { ok: true, ignored: true });
    if (!parsed.ok) {
      await answerCallback(cfg, parsed.callbackId, "This button is not for that chat.", fetch);
      return json(200, { ok: false, reason: parsed.reason });
    }
    try {
      const result = await decideReview(parsed.action, parsed.paymentId, request);
      await answerCallback(cfg, parsed.callbackId, result.summary, fetch);
      return json(200, { ok: true, action: parsed.action, paymentId: parsed.paymentId, summary: result.summary });
    } catch (error) {
      const message = error instanceof DemoError ? error.message : "The owner transaction did not finish.";
      await answerCallback(cfg, parsed.callbackId, message, fetch);
      return json(200, { ok: false, error: { code: error.code || "demo_failed", message } });
    }
  } catch (error) {
    return fromError(error);
  }
}
