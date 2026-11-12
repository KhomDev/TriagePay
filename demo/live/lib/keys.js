// Decide which access keys a cleanup transaction should delete.
// The pay-only key for the current wallet stays. A full-access key stays
// unless the caller opts in, and it is never removed when no pay key
// would remain: that would lock the account when the allowance runs out.

function permissionOf(key) {
  return key && key.access_key && key.access_key.permission;
}

function isPayKey(key, walletId) {
  const permission = permissionOf(key);
  if (!permission || typeof permission !== "object" || !permission.FunctionCall) return false;
  const call = permission.FunctionCall;
  const methods = Array.isArray(call.method_names) ? call.method_names : [];
  return call.receiver_id === walletId && methods.length === 1 && methods[0] === "pay";
}

function allowance(key) {
  const call = permissionOf(key).FunctionCall;
  try {
    return BigInt(call.allowance || 0);
  } catch {
    return 0n;
  }
}

export function selectKeysToDelete(keys, options) {
  const list = Array.isArray(keys) ? keys : [];
  const walletId = options.walletId;
  const payKeys = list.filter((key) => isPayKey(key, walletId));
  let keep = null;
  if (options.keepPublicKey) {
    keep = payKeys.find((key) => key.public_key === options.keepPublicKey) || null;
  }
  if (!keep && payKeys.length) {
    keep = payKeys.slice().sort((a, b) => (allowance(a) > allowance(b) ? -1 : 1))[0];
  }
  const drop = [];
  for (const key of list) {
    if (keep && key.public_key === keep.public_key) continue;
    const full = permissionOf(key) === "FullAccess";
    if (full && !options.dropFullAccess) continue;
    if (full && !keep) continue;
    drop.push(key.public_key);
  }
  return {
    keep: keep ? keep.public_key : null,
    drop,
  };
}
