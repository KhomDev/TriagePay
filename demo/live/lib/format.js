// Integer token amounts. No floats: a 24-decimal value does not fit in Number.

export function formatAtomic(atomic, decimals) {
  const value = BigInt(atomic);
  const scale = 10n ** BigInt(decimals);
  const negative = value < 0n;
  const abs = negative ? -value : value;
  const whole = abs / scale;
  let frac = (abs % scale).toString().padStart(Number(decimals), "0");
  frac = frac.replace(/0+$/, "");
  const text = Number(decimals) > 0 && frac ? `${whole}.${frac}` : whole.toString();
  return humanDust(negative ? `-${text}` : text);
}

function humanDust(text) {
  const match = /^(-?)(\d+)\.(\d+)$/.exec(text);
  if (!match) return text;
  const dust = /^(\d*?)(0{8,})([1-9]\d{0,4})$/.exec(match[3]);
  if (!dust || dust[3] !== "1") return text;
  const head = dust[1].replace(/0+$/, "");
  const shown = head ? `${match[1]}${match[2]}.${head}` : `${match[1]}${match[2]}`;
  return `${shown} wNEAR + 1 yocto`;
}

const TOKEN_SYMBOLS = {
  "wrap.near": "wNEAR",
  "wrap.testnet": "wNEAR",
};

export function tokenSymbol(accountId) {
  return TOKEN_SYMBOLS[String(accountId || "").toLowerCase()] || null;
}

export function formatAmount(atomic, decimals, symbol) {
  const text = formatAtomic(atomic, decimals);
  if (!symbol || text.includes(symbol)) return text;
  return `${text} ${symbol}`;
}

export function nsToIso(ns) {
  if (ns == null || ns === "") return null;
  const ms = typeof ns === "string" ? Number(BigInt(ns) / 1000000n) : Math.round(Number(ns) / 1e6);
  if (!Number.isFinite(ms)) return null;
  return new Date(ms).toISOString();
}

export function nsToMs(ns) {
  if (ns == null || ns === "") return null;
  if (typeof ns === "string") return Number(BigInt(ns) / 1000000n);
  const ms = Math.round(Number(ns) / 1e6);
  return Number.isFinite(ms) ? ms : null;
}
