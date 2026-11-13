// One function for the routes added after the original seven, so the
// Hobby plan stays at or under 12 functions. /api/telegram.js stays its
// own file. vercel.json rewrites the public paths here.

import { fail } from "../lib/http.js";
import { POST as agent } from "../routes/agent.js";
import { GET as drainGet, POST as drainPost } from "../routes/drain.js";
import { GET as killGet, POST as killPost } from "../routes/kill.js";
import { GET as og } from "../routes/og.js";
import { GET as share } from "../routes/share.js";
import { GET as replay } from "../routes/replay.js";
import { GET as honeypot } from "../routes/honeypot.js";
import { POST as policy } from "../routes/policy.js";
import { GET as passport, og as passportOg } from "../routes/passport.js";
import { GET as siteOg } from "../routes/site-og.js";
import { passportPage, replayPage } from "../routes/pages.js";
import { GET as home, boardGet } from "../routes/home.js";

const NAMES = new Set(["agent", "drain", "kill", "og", "share", "replay", "honeypot", "policy", "passport", "passport-og", "site-og", "replay-page", "passport-page", "home", "board"]);

const GETS = {
  drain: drainGet,
  kill: killGet,
  og,
  share,
  replay,
  honeypot,
  passport,
  "passport-og": passportOg,
  "site-og": siteOg,
  "replay-page": replayPage,
  "passport-page": passportPage,
  home,
  board: boardGet,
};

const POSTS = {
  agent,
  drain: drainPost,
  kill: killPost,
  policy,
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
