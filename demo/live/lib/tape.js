// Latest real pay calls for the first-fold ticker. Hashes only, no prose
// from the chain is treated as HTML.

const HASH = /^[A-Za-z0-9]{8,88}$/;

export function txHref(item) {
  const hash = item && item.hash;
  if (!HASH.test(String(hash || ""))) return "";
  const base = item.network === "mainnet"
    ? "https://nearblocks.io/txns/"
    : "https://testnet.nearblocks.io/txns/";
  return base + hash;
}

export function tapeFromRows(groups) {
  const seen = new Set();
  const items = [];
  for (const group of groups || []) {
    const network = group && group.network === "mainnet" ? "mainnet" : "testnet";
    for (const row of (group && group.rows) || []) {
      if (!row || !row.txHash || !(row.methods || []).includes("pay")) continue;
      if (!HASH.test(row.txHash) || seen.has(row.txHash)) continue;
      seen.add(row.txHash);
      items.push({
        hash: row.txHash,
        network,
        paid: Boolean(row.success),
        atMs: Number.isFinite(row.atMs) ? row.atMs : 0,
      });
    }
  }
  items.sort((a, b) => b.atMs - a.atMs);
  return items.slice(0, 6);
}

export function asOfClock(iso) {
  const when = new Date(iso || 0);
  if (Number.isNaN(when.getTime())) return "";
  const hh = String(when.getUTCHours()).padStart(2, "0");
  const mm = String(when.getUTCMinutes()).padStart(2, "0");
  return `as of ${hh}:${mm} UTC`;
}

function esc(value) {
  return String(value || "").replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#39;",
  }[ch]));
}

export function tapeHtml(rows) {
  return (rows || []).map((item) => {
    const href = txHref(item);
    if (!href) return "";
    const word = item.paid ? "PAID" : "REFUSED";
    const kind = item.paid ? "paid" : "refused";
    const net = item.network === "mainnet" ? "mainnet" : "testnet";
    return `<li><a href="${esc(href)}" rel="noopener" target="_blank"><span class="${kind}">${word}</span> ${esc(item.hash.slice(0, 8))} · ${net}</a></li>`;
  }).join("");
}

function replaceParagraph(html, id, inner) {
  return html.replace(
    new RegExp(`(<p id="${id}"[^>]*>)[\\s\\S]*?(</p>)`),
    `$1${inner}$2`,
  );
}

export function fillHome(html, board) {
  const source = board || {};
  let next = html;
  if (source.paid != null) next = replaceParagraph(next, "num-paid", esc(source.paid));
  if (source.refused != null) next = replaceParagraph(next, "num-refused", esc(source.refused));
  if (source.protectedDisplay) next = replaceParagraph(next, "num-protected", esc(source.protectedDisplay));
  if (source.agents != null) next = replaceParagraph(next, "num-agents", esc(source.agents));
  const clock = asOfClock(source.asOf);
  if (clock) {
    next = next.replace(
      /<p id="num-source" class="hint">[\s\S]*?<\/p>/,
      `<p id="num-source" class="hint" data-asof="${esc(source.asOf || "")}"><a href="https://nearblocks.io" rel="noopener" target="_blank">Live from NearBlocks</a> · ${esc(clock)}</p>`,
    );
  }
  if (source.attemptsRemaining != null) {
    next = replaceParagraph(next, "attempt-left", `${esc(source.attemptsRemaining)} testnet attempts left today`);
  }
  const items = tapeHtml(source.recent);
  if (items) {
    next = next.replace(
      /<ol id="tape" class="tape"[^>]*>[\s\S]*?<\/ol>/,
      `<ol id="tape" class="tape" aria-label="Latest on-chain decisions">${items}</ol>`,
    );
  }
  return next;
}
