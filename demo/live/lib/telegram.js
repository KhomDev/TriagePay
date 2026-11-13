// Owner messages for a held payment. The webhook checks the secret header
// Telegram sends, then the chat id, before any owner transaction.

import { createHash, timingSafeEqual } from "node:crypto";

export function telegramConfig(env = process.env) {
  const token = String(env.TELEGRAM_BOT_TOKEN || "").trim();
  const chatId = String(env.TELEGRAM_OWNER_CHAT_ID || "").trim();
  const secret = String(env.TELEGRAM_WEBHOOK_SECRET || "").trim();
  return {
    token,
    chatId,
    secret,
    configured: Boolean(token && chatId),
    webhookConfigured: Boolean(token && chatId && secret),
  };
}

function same(left, right) {
  const a = Buffer.from(String(left));
  const b = Buffer.from(String(right));
  if (a.length !== b.length || a.length === 0) return false;
  return timingSafeEqual(a, b);
}

export function verifyTelegramHeader(request, cfg = telegramConfig()) {
  if (!cfg.webhookConfigured) {
    return {
      ok: false,
      status: 503,
      code: "telegram_not_configured",
      message: "Set TELEGRAM_BOT_TOKEN, TELEGRAM_OWNER_CHAT_ID, and TELEGRAM_WEBHOOK_SECRET. The webhook is off until then.",
    };
  }
  const header = request.headers.get("x-telegram-bot-api-secret-token") || "";
  if (!same(header, cfg.secret)) {
    return { ok: false, status: 401, code: "telegram_unauthorized", message: "The Telegram secret token did not match." };
  }
  return { ok: true };
}

export function parseCallback(update, ownerChatId) {
  const query = update && update.callback_query;
  if (!query) return { ignore: true };
  const chatId = query.message && query.message.chat ? String(query.message.chat.id) : "";
  if (!same(chatId, String(ownerChatId))) {
    return { ok: false, reason: "chat", callbackId: query.id || null };
  }
  const data = String(query.data || "");
  const match = /^(a|r):(\d{1,20})$/.exec(data);
  if (!match) return { ok: false, reason: "data", callbackId: query.id || null };
  return {
    ok: true,
    action: match[1] === "a" ? "approve" : "reject",
    paymentId: match[2],
    callbackId: query.id || null,
    messageId: query.message && query.message.message_id,
    chatId,
  };
}

async function telegramPost(cfg, method, body, fetchImpl) {
  const response = await fetchImpl(`https://api.telegram.org/bot${cfg.token}/${method}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(4000),
  });
  if (!response.ok) {
    try {
      await response.body?.cancel?.();
    } catch {
      // Status only.
    }
    throw new Error(`Telegram HTTP ${response.status}`);
  }
  return response.json();
}

export async function notifyHold({ payee, amountDisplay, paymentId, explorerUrl, fetchImpl = fetch, env = process.env }) {
  const cfg = telegramConfig(env);
  if (!cfg.configured) {
    return {
      configured: false,
      sent: false,
      reason: "Telegram is not configured. Set TELEGRAM_BOT_TOKEN and TELEGRAM_OWNER_CHAT_ID.",
    };
  }
  const lines = [
    "TriagePay held a payment.",
    `Payee: ${payee || "unknown"}`,
    `Amount: ${amountDisplay || "unknown"}`,
    paymentId ? `Payment id: ${paymentId}` : "Payment id: open the demo and read the queue.",
  ];
  if (explorerUrl) lines.push(explorerUrl);
  const payload = {
    chat_id: cfg.chatId,
    text: lines.join("\n").slice(0, 3500),
  };
  if (paymentId) {
    payload.reply_markup = {
      inline_keyboard: [[
        { text: "Approve", callback_data: `a:${paymentId}` },
        { text: "Reject", callback_data: `r:${paymentId}` },
      ]],
    };
  }
  try {
    await telegramPost(cfg, "sendMessage", payload, fetchImpl);
    return { configured: true, sent: true, reason: null };
  } catch {
    return { configured: true, sent: false, reason: "Telegram did not accept the message." };
  }
}

export async function answerCallback(cfg, callbackId, text, fetchImpl = fetch) {
  if (!cfg.token || !callbackId) return;
  try {
    await telegramPost(cfg, "answerCallbackQuery", {
      callback_query_id: callbackId,
      text: String(text || "").slice(0, 180),
    }, fetchImpl);
  } catch {
    // The owner still has the page if the toast fails.
  }
}

export function secretHint(secret) {
  if (!secret) return null;
  return createHash("sha256").update(secret).digest("hex").slice(0, 8);
}
