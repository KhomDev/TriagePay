import QRCode from "qrcode";
import { setSpending } from "../lib/demo.js";
import { DemoError, fail, fromError, json, readJson } from "../lib/http.js";
import {
  consumeNonce,
  killSecret,
  killTtlMs,
  mintKillToken,
  publicOrigin,
  releaseNonce,
  verifyKillToken,
} from "../lib/kill.js";
import { storeFromEnv } from "../lib/store.js";

function escapeHtml(value) {
  return String(value || "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#39;",
  }[ch]));
}

function page(title, body, status) {
  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
</head>
<body style="margin:0;background:#07080c;color:#f4f1e8;font:18px/1.5 sans-serif">
<main style="max-width:36rem;margin:12vh auto;padding:24px">
<p style="letter-spacing:.16em;color:#e2ff54">TRIAGEPAY KILL SWITCH</p>
<h1>${escapeHtml(title)}</h1>
<p>${escapeHtml(body)}</p>
<p>built at NEAR HACKATHON @NEARProtocol #NEARHackathon</p>
</main>
</body>
</html>`;
  return new Response(html, {
    status,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });
}

export async function GET(request) {
  const token = new URL(request.url).searchParams.get("t") || "";
  if (!token || token.length > 300) return page("Link rejected", "That link is not a kill switch.", 400);
  const secret = killSecret();
  if (!secret) return page("Kill switch is off", "Set KILL_SWITCH_SECRET on the server.", 503);
  const now = Date.now();
  const checked = verifyKillToken(token, secret, now);
  if (!checked.ok) {
    const text = checked.reason === "expired" ? "This link has expired." : "This link is not valid.";
    return page("Link rejected", text, 400);
  }
  let claimed;
  try {
    claimed = await consumeNonce(checked.nonce, Math.max(1000, checked.exp - now));
  } catch {
    return page("Kill switch store failed", "The single-use store did not answer, so I did not revoke.", 503);
  }
  if (!claimed.ok) return page("Already used", "This link was already used.", 409);
  try {
    const result = await setSpending("revoke", request);
    const extra = result.chain && result.chain.explorerUrl ? ` ${result.chain.explorerUrl}` : "";
    return page(
      result.changed ? "Agent revoked" : "Agent already inactive",
      `${result.summary || "The owner revoke finished."}${extra}`,
      200,
    );
  } catch (error) {
    await releaseNonce(claimed);
    const message = error instanceof DemoError
      ? error.message
      : "The revoke did not finish. Open the link again.";
    return page("Revoke did not finish", message, error instanceof DemoError ? error.status : 500);
  }
}

export async function POST(request) {
  try {
    await readJson(request);
    const secret = killSecret();
    if (!secret) {
      return fail(503, "kill_switch_off", "Set KILL_SWITCH_SECRET on the server. Until then I cannot mint a revoke link.");
    }
    const now = Date.now();
    const minted = mintKillToken({ secret, nowMs: now, ttlMs: killTtlMs() });
    const url = `${publicOrigin(request)}/api/kill?t=${encodeURIComponent(minted.token)}`;
    const qrSvg = await QRCode.toString(url, {
      type: "svg",
      margin: 1,
      color: { dark: "#e2ff54", light: "#07080c" },
    });
    return json(200, {
      url,
      expiresAt: new Date(minted.exp).toISOString(),
      qrSvg,
      singleUse: storeFromEnv() ? "store" : "memory",
    });
  } catch (error) {
    return fromError(error);
  }
}
