// Process-local record of attempts this instance signed. Public fields only.
// The dashboard merges this with the chain index, which survives a cold start.

import { readFileSync, writeFileSync } from "node:fs";
import { redact } from "./http.js";

const MAX = 40;
const memory = [];
const file = process.env.TRIAGEPAY_JOURNAL || "/tmp/triagepay-demo-journal.json";
let loaded = false;

function load() {
  if (loaded) return;
  loaded = true;
  try {
    const parsed = JSON.parse(readFileSync(file, "utf8"));
    if (Array.isArray(parsed)) memory.push(...parsed.slice(0, MAX));
  } catch {
    // A missing journal is the normal cold start.
  }
}

export function remember(entry) {
  load();
  const dumped = redact(JSON.stringify(entry));
  const safe = JSON.parse(dumped);
  memory.unshift(safe);
  if (memory.length > MAX) memory.length = MAX;
  try {
    writeFileSync(file, JSON.stringify(memory));
  } catch {
    // /tmp can be read-only in some sandboxes. Memory still serves this process.
  }
  return safe;
}

export function recall() {
  load();
  return memory.slice();
}

export function resetJournalForTests() {
  memory.length = 0;
  loaded = true;
}
