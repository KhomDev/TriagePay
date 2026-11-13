// A short cache shared by every function in this region when Vercel
// Runtime Cache is present. Otherwise this process keeps the value.
// Neither path is a paid database.

const memory = new Map();
let remotePromise;

function remoteCache() {
  if (!process.env.VERCEL) return Promise.resolve(null);
  if (!remotePromise) {
    remotePromise = import("@vercel/functions")
      .then((mod) => (typeof mod.getCache === "function" ? mod.getCache({ namespace: "triagepay" }) : null))
      .catch(() => null);
  }
  return remotePromise;
}

// Remote wins when it answers, so a write from another instance is visible.
export async function cacheRead(key) {
  const remote = await remoteCache();
  if (remote) {
    try {
      const value = await remote.get(key);
      if (value != null) {
        memory.set(key, { value, until: Date.now() + 5000 });
        return value;
      }
    } catch {
      // Fall through to this process.
    }
  }
  const local = memory.get(key);
  if (local && local.until > Date.now()) return local.value;
  return null;
}

export async function cacheGet(key) {
  const local = memory.get(key);
  if (local && local.until > Date.now()) return local.value;
  const remote = await remoteCache();
  if (!remote) return null;
  try {
    const value = await remote.get(key);
    if (value == null) return null;
    memory.set(key, { value, until: Date.now() + 15000 });
    return value;
  } catch {
    return null;
  }
}

export async function cacheSet(key, value, ttlSeconds) {
  memory.set(key, { value, until: Date.now() + ttlSeconds * 1000 });
  const remote = await remoteCache();
  if (!remote) return;
  try {
    await remote.set(key, value, { ttl: ttlSeconds, tags: ["triagepay"] });
  } catch {
    // The in-process copy is enough for this instance.
  }
}

export function resetRegionCache() {
  memory.clear();
  remotePromise = null;
}
