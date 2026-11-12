// Local server for the same functions Vercel runs. Static files come from public/.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { GET as health } from "./api/health.js";
import { GET as dashboard } from "./api/dashboard.js";
import { POST as run } from "./api/run.js";
import { POST as revoke } from "./api/revoke.js";
import { POST as review } from "./api/review.js";
import { GET as judgment } from "./api/judgment.js";
import { GET as cron } from "./api/cron.js";

const ROOT = fileURLToPath(new URL("./public/", import.meta.url));
const PORT = Number(process.env.PORT || 8787);
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
};

const routes = {
  "GET /api/health": health,
  "GET /api/dashboard": dashboard,
  "POST /api/run": run,
  "POST /api/revoke": revoke,
  "POST /api/review": review,
  "GET /api/judgment": judgment,
  "GET /api/cron": cron,
};

async function toRequest(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = Buffer.concat(chunks);
  const url = new URL(req.url, `http://127.0.0.1:${PORT}`);
  return new Request(url, {
    method: req.method,
    headers: req.headers,
    body: req.method === "GET" || req.method === "HEAD" ? undefined : body,
  });
}

async function send(res, response) {
  res.statusCode = response.status;
  response.headers.forEach((value, key) => res.setHeader(key, value));
  res.end(Buffer.from(await response.arrayBuffer()));
}

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://127.0.0.1:${PORT}`);
    const handler = routes[`${req.method} ${url.pathname}`];
    if (handler) {
      await send(res, await handler(await toRequest(req)));
      return;
    }
    if (req.method !== "GET" && req.method !== "HEAD") {
      res.statusCode = 405;
      res.end("method not allowed");
      return;
    }
    const requested = url.pathname === "/" ? "index.html" : url.pathname.replace(/^\/+/, "");
    const path = resolve(ROOT, requested);
    const root = resolve(ROOT);
    if (path !== root && !path.startsWith(root + sep)) {
      res.statusCode = 404;
      res.end("not found");
      return;
    }
    const body = await readFile(path);
    res.setHeader("content-type", TYPES[extname(path)] || "application/octet-stream");
    res.setHeader("cache-control", "no-store");
    res.end(req.method === "HEAD" ? undefined : body);
  } catch (error) {
    if (error && error.code === "ENOENT") {
      res.statusCode = 404;
      res.end("not found");
      return;
    }
    res.statusCode = 500;
    res.end("demo server failed");
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`TriagePay live demo at http://127.0.0.1:${PORT}`);
});
