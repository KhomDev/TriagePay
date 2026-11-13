import { fail } from "../lib/http.js";
import { sanitizeReceiptQuery } from "../lib/og-image.js";
import { SHARE_TAIL } from "../lib/receipt.js";
import { publicOrigin } from "../lib/kill.js";

function escapeHtml(value) {
  return String(value || "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#39;",
  }[ch]));
}

export async function GET(request) {
  const url = new URL(request.url);
  const fields = sanitizeReceiptQuery(url.searchParams);
  if (!fields) return fail(400, "bad_receipt", "The receipt fields were rejected.");
  const origin = publicOrigin(request);
  const image = `${origin}/api/og?${url.searchParams.toString()}`;
  const title = `TriagePay ${fields.headline}`;
  const description = `${fields.headline} ${fields.rule}. ${fields.amount || "an amount"} to ${fields.payee || "a payee"}. ${SHARE_TAIL}`;
  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<meta property="og:title" content="${escapeHtml(title)}">
<meta property="og:description" content="${escapeHtml(description)}">
<meta property="og:image" content="${escapeHtml(image)}">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="${escapeHtml(title)}">
<meta name="twitter:description" content="${escapeHtml(description)}">
<meta name="twitter:image" content="${escapeHtml(image)}">
</head>
<body style="margin:0;background:#07080c;color:#f4f1e8;font:18px/1.5 sans-serif">
<main style="max-width:40rem;margin:8vh auto;padding:24px">
<p style="letter-spacing:.16em;color:#e2ff54">TRIAGEPAY</p>
<h1>${escapeHtml(fields.headline)}</h1>
<p>${escapeHtml(fields.rule)} — ${escapeHtml(fields.ruleText)}</p>
<p>Amount ${escapeHtml(fields.amount || "—")}. Payee ${escapeHtml(fields.payee || "—")}.</p>
<p>${escapeHtml(fields.tried || "")}</p>
<p>${escapeHtml(SHARE_TAIL)}</p>
</main>
</body>
</html>`;
  return new Response(html, {
    status: 200,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "public, max-age=300" },
  });
}
