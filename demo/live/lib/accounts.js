// NEAR account ids as the wallet will see them. Extraction is for the
// scripted agent: it only pays an account that is written in the text.

const NAMED = /^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/;

export function isNearAccount(id) {
  if (typeof id !== "string") return false;
  const value = id.trim().toLowerCase();
  if (value.length < 2 || value.length > 64) return false;
  if (/^[0-9a-f]{64}$/.test(value)) return true;
  if (!value.includes(".")) return false;
  return NAMED.test(value);
}

export function isAtomicAmount(value) {
  return typeof value === "string" && /^[0-9]{1,40}$/.test(value);
}

export function extractAccount(text) {
  const source = String(text || "");
  const named = source.match(/(?:^|[^a-z0-9._-])((?:[a-z0-9](?:[a-z0-9_-]{0,61}[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9_-]{0,61}[a-z0-9])?)(?![a-z0-9._-])/i);
  if (named && isNearAccount(named[1])) return named[1].toLowerCase();
  const implicit = source.match(/(?:^|[^0-9a-f])([0-9a-f]{64})(?![0-9a-f])/i);
  if (implicit && isNearAccount(implicit[1])) return implicit[1].toLowerCase();
  return null;
}
