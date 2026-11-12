import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const html = readFileSync(join(ROOT, "public", "index.html"), "utf8");
const css = readFileSync(join(ROOT, "public", "style.css"), "utf8");
const script = readFileSync(join(ROOT, "public", "app.js"), "utf8");

test("the judge path and the hackathon marks are on the page", () => {
  assert.match(html, /Launch prompt-injection attack/);
  assert.match(html, /built at NEAR HACKATHON/);
  assert.match(html, /#NEARHackathon/);
  assert.match(html, /@NEARProtocol/);
  assert.match(html, /TRIAGEPAY_OWNER_SECRET_KEY/);
  assert.match(html, /shows <strong>warn<\/strong>/);
  assert.match(css, /prefers-reduced-motion/);
  assert.match(html, /href="style.css"/);
});

test("the page script does not parse chain data as HTML", () => {
  for (const sink of ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval(", "new Function"]) {
    assert.equal(script.includes(sink), false, sink);
  }
  assert.match(script, /injected-payee/);
  assert.match(script, /\/api\/judgment/);
  assert.match(script, /\/api\/review/);
});
