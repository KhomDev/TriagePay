// Build public/app.min.js from public/app.js.
//
//   node scripts/build-client.mjs           write the bundle
//   node scripts/build-client.mjs --check   exit 1 if the bundle is stale
//
// The bundle is the source with comment-only lines, blank lines and
// indentation removed, plus a banner naming the source's sha256, so a test
// can tell when someone edited app.js and forgot to rebuild. Trimming is
// safe because app.js has no template literals; the build refuses a source
// that has one. No third-party tool is involved.

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE = join(ROOT, "public", "app.js");
const BUNDLE = join(ROOT, "public", "app.min.js");

export function sourceHash(text) {
  return createHash("sha256").update(text).digest("hex");
}

export function bundle(text) {
  if (text.includes("`")) {
    throw new Error("app.js has a template literal; trimming lines could change it");
  }
  const body = text
    .split(/\r?\n/)
    .filter((line) => line.trim() !== "" && !/^\s*\/\/(?!#)/.test(line))
    .map((line) => line.trim())
    .join("\n");
  return `/* built from app.js sha256:${sourceHash(text)} by scripts/build-client.mjs */\n${body}\n`;
}

export function bannerHash(bundled) {
  const match = /^\/\* built from app\.js sha256:([0-9a-f]{64}) /.exec(bundled);
  return match ? match[1] : null;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const text = readFileSync(SOURCE, "utf8");
  if (process.argv.includes("--check")) {
    const current = readFileSync(BUNDLE, "utf8");
    if (bannerHash(current) !== sourceHash(text)) {
      console.error("app.min.js is stale: run node scripts/build-client.mjs");
      process.exit(1);
    }
    console.log("app.min.js matches app.js");
  } else {
    writeFileSync(BUNDLE, bundle(text), "utf8");
    console.log("wrote public/app.min.js");
  }
}
