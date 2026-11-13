import { publicOrigin } from "../lib/kill.js";
import { SHARE_TAIL } from "../lib/receipt.js";

function escapeHtml(value) {
  return String(value || "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#39;",
  }[ch]));
}

function page({ title, description, image, script }) {
  return new Response(`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark">
<title>${escapeHtml(title)}</title>
<meta name="description" content="${escapeHtml(description)}">
<meta property="og:title" content="${escapeHtml(title)}">
<meta property="og:description" content="${escapeHtml(description)}">
<meta property="og:image" content="${escapeHtml(image)}">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="${escapeHtml(title)}">
<meta name="twitter:description" content="${escapeHtml(description)}">
<meta name="twitter:image" content="${escapeHtml(image)}">
<link rel="icon" href="/mark.svg" type="image/svg+xml">
<link rel="stylesheet" href="/style.css">
<script src="${script}" defer></script>
</head>
<body>
<a class="skip-link" href="#main">Skip to content</a>
<header class="nav">
  <a class="brand" href="/"><img src="/mark.svg" width="28" height="28" alt=""><span>TriagePay</span></a>
  <p class="tag">built at NEAR HACKATHON</p>
</header>
<main id="main" class="sheet"></main>
<footer class="site"><p>${escapeHtml(SHARE_TAIL)}</p></footer>
</body>
</html>`, {
    status: 200,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });
}

export function replayPage(request) {
  const origin = publicOrigin(request);
  const tx = new URL(request.url).searchParams.get("tx") || "";
  return page({
    title: tx ? `Replay ${tx.slice(0, 8)}` : "Replay",
    description: "A ten-second replay of a wallet refusal or hold. The transaction link is the chain record.",
    image: `${origin}/api/site-og`,
    script: "/replay.js",
  });
}

export function passportPage(request) {
  const origin = publicOrigin(request);
  const account = new URL(request.url).searchParams.get("account") || "";
  return page({
    title: account ? `Agent ${account}` : "Agent passport",
    description: "Limits, allowlist, and recent transactions for one agent, read from the wallet and the indexer.",
    image: `${origin}/api/passport-og?account=${encodeURIComponent(account)}`,
    script: "/agent.js",
  });
}
