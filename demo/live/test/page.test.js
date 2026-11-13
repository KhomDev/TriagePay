import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const html = readFileSync(join(ROOT, "public", "shell.html"), "utf8");
const scoreboard = JSON.parse(readFileSync(join(ROOT, "public", "scoreboard.json"), "utf8"));
const css = readFileSync(join(ROOT, "public", "style.css"), "utf8");
const script = readFileSync(join(ROOT, "public", "app.js"), "utf8");

test("the judge path and the hackathon marks are on the page", () => {
  assert.match(html, /Launch the attack/);
  assert.match(html, /Try to drain it/);
  assert.match(html, /The agent signs\. The contract decides\./);
  assert.match(html, /Agents pay per call over x402 on NEAR, and the wallet is the security layer that makes that safe/);
  assert.match(html, /id="net-mainnet"/);
  assert.match(html, /id="opt-live"/);
  assert.equal(/vercel\.live/.test(html), false);
  assert.equal(/CFutaZM9/.test(html), false);
  assert.match(html, /id="attempt-left"/);
  assert.match(html, new RegExp(scoreboard.attemptsRemaining + " testnet attempts left today"));
  assert.match(html, new RegExp(`id="num-paid" class="stat">${scoreboard.paid}`));
  assert.match(html, new RegExp(`id="num-refused" class="stat">${scoreboard.refused}`));
  assert.match(css, /replay-steps \.trace/);
  assert.match(css, /word-break: break-word/);
  assert.match(html, /The contract has not decided yet/);
  assert.equal(/The three proofs/.test(html), false);
  assert.match(html, /1000-yocto mainnet pair/);
  assert.match(html, /6F3jNamjerT69EMnxaQVy7WRdZuscj7t53V1oceTU4Yj/);
  assert.match(html, /5BCGjxC4CkPSy8iv876cP65xxs2LRWwcWKFqmcVJEyEw/);
  assert.match(html, /Earlier proof, paid call/);
  assert.match(html, /id="tape"/);
  assert.match(script, /\/api\/board/);
  assert.match(script, /scoreboard\.json/);
  assert.match(script, /as of /);
  assert.match(script, /triagepay-board-floor/);
  assert.match(script, /There is no transaction link/);
  assert.match(html, /Judge in 60 seconds/);
  assert.match(html, /Why it's different/);
  assert.match(html, /github.com\/KhomDev\/TriagePay\/blob\/main\/docs\/claims-audit.md/);
  assert.match(html, /built at NEAR HACKATHON/);
  assert.match(html, /#NEARHackathon/);
  assert.match(html, /@NEARProtocol/);
  assert.match(html, /TRIAGEPAY_OWNER_SECRET_KEY/);
  assert.match(html, /shows <strong>warn<\/strong>/);
  assert.match(css, /prefers-reduced-motion/);
  assert.match(html, /href="\/style.css"/);
  assert.match(html, /src="\/app\.min\.js"/);
  assert.match(css, /\.tape a \{[\s\S]*min-height: 48px/);
  assert.match(css, /\.tape \{[\s\S]*grid-template-columns: repeat\(3/);
});

test("Check attestation re-enables Launch when it finishes", () => {
  const start = script.indexOf("async function onAttest");
  const end = script.indexOf("async function onDrain");
  const attest = script.slice(start, end);
  assert.match(attest, /state\.busy = false/);
  assert.match(attest, /syncControls\(\)/);
  assert.match(script, /function syncControls\(\)/);
  assert.match(script, /\$\("btn-launch"\)\.disabled = !ready \|\| busy \|\| blocked/);
});

test("the page script does not parse chain data as HTML", () => {
  for (const sink of ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval(", "new Function"]) {
    assert.equal(script.includes(sink), false, sink);
  }
  assert.match(script, /injected-payee/);
  assert.match(script, /\/api\/judgment/);
  assert.match(script, /\/api\/review/);
  assert.match(script, /\/api\/drain/);
  assert.match(script, /\/api\/kill/);
  assert.match(script, /built at NEAR HACKATHON @NEARProtocol #NEARHackathon/);
  assert.match(html, /Drain Me/);
  assert.match(html, /Kill switch/);
  assert.match(html, /Launch the attack/);
  assert.match(script, /approvalThresholdDisplay/);
});
