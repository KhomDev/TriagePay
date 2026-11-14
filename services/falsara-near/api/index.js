// The single serverless function. vercel.json rewrites every API path here.
import { loadConfig } from "../lib/config.js";
import { handle } from "../lib/handler.js";

const config = loadConfig();

function withPath(request) {
  const url = new URL(request.url);
  const path = url.searchParams.get("path");
  if (path) {
    url.pathname = "/" + path.replace(/^\/+/, "");
    url.searchParams.delete("path");
  }
  return new Request(url, request);
}

export function GET(request) {
  return handle(withPath(request), { config });
}

export function POST(request) {
  return handle(withPath(request), { config });
}

export function OPTIONS(request) {
  return handle(withPath(request), { config });
}
