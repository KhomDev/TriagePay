// One function for the routes added after the original seven, so the
// Hobby plan stays at or under 12 functions. /api/telegram.js stays its
// own file. vercel.json rewrites the public paths here.

import { fail } from "../lib/http.js";
import { POST as agent } from "../routes/agent.js";
import { GET as drainGet, POST as drainPost } from "../routes/drain.js";
import { GET as killGet, POST as killPost } from "../routes/kill.js";
import { GET as og } from "../routes/og.js";
import { GET as share } from "../routes/share.js";

const NAMES = new Set(["agent", "drain", "kill", "og", "share"]);

const GETS = {
  drain: drainGet,
  kill: killGet,
  og,
  share,
};

const POSTS = {
  agent,
  drain: drainPost,
  kill: killPost,
};

export function routeOf(request) {
  const url = new URL(request.url);
  const query = url.searchParams.get("route");
  if (query && NAMES.has(query)) return query;
  const last = url.pathname.split("/").filter(Boolean).pop();
  if (last && NAMES.has(last)) return last;
  return null;
}

function dispatch(table, request) {
  const name = routeOf(request);
  const handler = name && table[name];
  if (!handler) return fail(404, "not_found", "That demo route is not on this function.");
  return handler(request);
}

export function GET(request) {
  return dispatch(GETS, request);
}

export function POST(request) {
  return dispatch(POSTS, request);
}
