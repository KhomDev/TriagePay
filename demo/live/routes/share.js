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
  const imageParams = new URLSearchParams();
  imageParams.set("kind", fields.kind);
  if (fields.rule) imageParams.set("rule", fields.rule);
  if (fields.payee) imageParams.set("payee", fields.payee);
  if (fields.amount) imageParams.set("amount", fields.amount);
  if (fields.tx) imageParams.set("tx", fields.tx);
  if (fields.tried) imageParams.set("tried", fields.tried);
  const image = `${origin}/api/og?${imageParams.toString()}`;
  const title = `TriagePay ${fields.headline}`;
  const movement = fields.amount && fields.payee ? `${fields.amount} to ${fields.payee}` : (fields.amount || fields.payee || "");
  const description = `${[fields.headline, fields.rule && fields.rule !== fields.headline ? fields.rule : ""].filter(Boolean).join(" ")}${movement ? `. ${movement}` : ""}. ${SHARE_TAIL}`;
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
<link rel="stylesheet" href="/style.css">
</head>
<body class="plain-page">
<main class="plain-main">
<p class="plain-kicker">TRIAGEPAY</p>
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
