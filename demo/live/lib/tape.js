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

// Plain words for the contract's refusal codes, as a reader would say them.
export const REASON = {
  ERR_PAYEE_NOT_ALLOWED: "Payee not on the allowlist",
  ERR_PER_CALL_LIMIT: "Over the per-call cap",
  ERR_DAILY_LIMIT: "Over the daily cap",
  ERR_AGENT_INACTIVE: "Agent revoked",
  ERR_PAUSED: "Wallet paused",
  ERR_TOKEN_NOT_ALLOWED: "Token not allowed",
  ERR_QUEUE_FULL: "Review queue full",
};
const ACCOUNT = /^[a-z0-9._-]{2,64}$/;

/** The words one ticker row shows. Codes and payees come from the chain. */
export function tapeWords(item) {
  const held = Boolean(item && item.held);
  const status = held ? "Held" : item && item.paid ? "Paid" : "Refused";
  const kind = held ? "held" : item && item.paid ? "paid" : "refused";
  const code = item && /^ERR_[A-Z_]+$/.test(String(item.code || "")) ? String(item.code) : null;
  const reason = held
    ? "Waiting for the owner"
    : item && item.paid
      ? "Allowlisted payee, inside the caps"
      : (code && (REASON[code] || code)) || "Refused by the contract";
  const payee = item && ACCOUNT.test(String(item.payee || "")) ? String(item.payee) : null;
  const network = item && item.network === "mainnet" ? "Mainnet" : "Testnet";
  return { status, kind, code, reason, payee, network };
}

export function tapeHtml(rows) {
  return (rows || []).map((item) => {
    const href = txHref(item);
    if (!href) return "";
    const w = tapeWords(item);
    const payee = w.payee ? `<span class="tape-payee">${esc(w.payee)}</span>` : "";
    return `<li class="is-${w.kind}"><a href="${esc(href)}" rel="noopener" target="_blank">`
      + `<span class="tape-status">${w.status}</span>`
      + `<span class="tape-reason">${esc(w.reason)}</span>`
      + payee
      + `<span class="tape-net">${w.network}</span></a></li>`;
  }).join("");
}

function replaceParagraph(html, id, inner) {
  return html.replace(
    new RegExp(`(<p id="${id}"[^>]*>)[\\s\\S]*?(</p>)`),
    `$1${inner}$2`,
  );
}

export const FALLBACK_TAPE = [
  { hash: "FfmGZyE4j8LK3uPzQCAcXbLwuzTgPNy76LasjoHVamHH", network: "mainnet", paid: false, atMs: 2 },
  { hash: "9YCqzVgVNvaED7twQgtJvhUeCHMrnDiDAwKdbRHxnQWp", network: "mainnet", paid: true, atMs: 3 },
  { hash: "8y6sEwL8i5sP2aMKo5HS3WGZtZ8NMDP9MUdywYAmVnN4", network: "testnet", paid: false, atMs: 1 },
  { hash: "4oxycWdvo9M4YWoq9mg2y4FXL7AZYQY6McvcPJQYJejq", network: "testnet", paid: true, atMs: 1 },
];

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
  const rows = source.recent && source.recent.length ? source.recent : FALLBACK_TAPE;
  const items = tapeHtml(rows);
  const key = rows.map((item) => item.hash).join(",");
  next = next.replace(
    /<ol id="tape" class="tape"[^>]*>[\s\S]*?<\/ol>/,
    `<ol id="tape" class="tape" data-key="${esc(key)}" aria-label="Latest on-chain decisions">${items}</ol>`,
  );
  return next;
}
