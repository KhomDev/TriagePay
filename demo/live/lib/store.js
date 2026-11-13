// Shared counters for the hosted demo. Vercel KV and Upstash Redis expose
// the same REST shape. When neither is configured, the caller uses the
// chain index instead of this module.

export function storeFromEnv(env = process.env) {
  const url = String(env.UPSTASH_REDIS_REST_URL || env.KV_REST_API_URL || "").trim();
  const token = String(env.UPSTASH_REDIS_REST_TOKEN || env.KV_REST_API_TOKEN || "").trim();
  if (!url || !token) return null;
  return upstashStore(url.replace(/\/$/, ""), token);
}

export function upstashStore(url, token, fetchImpl = fetch) {
  async function command(args) {
    const response = await fetchImpl(url, {
      method: "POST",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
      },
      body: JSON.stringify(args),
      signal: AbortSignal.timeout(4000),
    });
    if (!response.ok) {
      throw new Error(`shared store HTTP ${response.status}`);
    }
    const body = await response.json();
    if (body.error) throw new Error(`shared store ${body.error}`);
    return body.result;
  }
  return {
    async incr(key) {
      return Number(await command(["INCR", key]));
    },
    async decr(key) {
      return Number(await command(["DECR", key]));
    },
    async expire(key, seconds) {
      return Number(await command(["EXPIRE", key, String(seconds)]));
    },
    async pttl(key) {
      return Number(await command(["PTTL", key]));
    },
    async setNxPx(key, value, px) {
      return command(["SET", key, String(value), "NX", "PX", String(px)]);
    },
    async get(key) {
      const value = await command(["GET", key]);
      return value == null ? null : String(value);
    },
    async del(key) {
      return Number(await command(["DEL", key]));
    },
    async set(key, value) {
      return command(["SET", key, String(value)]);
    },
    async lpush(key, value) {
      return Number(await command(["LPUSH", key, String(value)]));
    },
    async ltrim(key, start, stop) {
      return command(["LTRIM", key, String(start), String(stop)]);
    },
    async lrange(key, start, stop) {
      const rows = await command(["LRANGE", key, String(start), String(stop)]);
      return Array.isArray(rows) ? rows.map((row) => String(row)) : [];
    },
  };
}

// Process-local stand-in used by tests and by the per-IP fallback.
export function memoryStore(now = () => Date.now()) {
  const data = new Map();
  const expiry = new Map();
  function live(key) {
    const exp = expiry.get(key);
    if (exp != null && exp <= now()) {
      data.delete(key);
      expiry.delete(key);
    }
  }
  return {
    async incr(key) {
      live(key);
      const next = Number(data.get(key) || 0) + 1;
      data.set(key, next);
      return next;
    },
    async decr(key) {
      live(key);
      const next = Number(data.get(key) || 0) - 1;
      data.set(key, next);
      return next;
    },
    async expire(key, seconds) {
      if (!data.has(key)) return 0;
      expiry.set(key, now() + seconds * 1000);
      return 1;
    },
    async pttl(key) {
      live(key);
      if (!data.has(key)) return -2;
      const exp = expiry.get(key);
      if (exp == null) return -1;
      return Math.max(0, exp - now());
    },
    async setNxPx(key, value, px) {
      live(key);
      if (data.has(key)) return null;
      data.set(key, value);
      expiry.set(key, now() + px);
      return "OK";
    },
    async get(key) {
      live(key);
      return data.has(key) ? String(data.get(key)) : null;
    },
    async del(key) {
      const had = data.delete(key);
      expiry.delete(key);
      return had ? 1 : 0;
    },
    async set(key, value) {
      live(key);
      data.set(key, String(value));
      return "OK";
    },
    async lpush(key, value) {
      live(key);
      const cur = data.get(key);
      const list = Array.isArray(cur) ? cur : [];
      list.unshift(String(value));
      data.set(key, list);
      return list.length;
    },
    async ltrim(key, start, stop) {
      live(key);
      const cur = data.get(key);
      if (!Array.isArray(cur)) return "OK";
      data.set(key, cur.slice(start, stop + 1));
      return "OK";
    },
    async lrange(key, start, stop) {
      live(key);
      const cur = data.get(key);
      if (!Array.isArray(cur)) return [];
      return cur.slice(start, stop + 1).map(String);
    },
  };
}
