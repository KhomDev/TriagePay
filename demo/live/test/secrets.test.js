import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { applyEnv, assertPublicConfig, readPublicConfig } from "../lib/config.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SECRET = /ed25519:[1-9A-HJ-NP-Za-km-z]{20,}/;

function filesUnder(dir) {
  const found = [];
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === ".keys" || name === ".vercel") continue;
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) found.push(...filesUnder(path));
    else found.push(path);
  }
  return found;
}

test("the public tree has no signing key", () => {
  const hits = [];
  for (const path of filesUnder(ROOT)) {
    if (path.endsWith(".wasm")) continue;
    const text = readFileSync(path);
    if (SECRET.test(text.toString("utf8"))) hits.push(path);
  }
  assert.deepEqual(hits, []);
});

test("config.json is public chain data", () => {
  const config = readPublicConfig();
  assertPublicConfig(config);
  assert.equal(config.network, "testnet");
  assert.equal(config.rpcUrl, "https://test.rpc.fastnear.com");
  assert.equal(config.accounts.token, "wrap.testnet");
  assert.equal(config.explorerTxBase, "https://testnet.nearblocks.io/txns/");
  assert.ok(BigInt(config.policy.allowedAmount) < BigInt(config.policy.perCallMax));
  assert.ok(BigInt(config.policy.perCallMax) <= BigInt(config.policy.dailyMax));
  assert.equal(config.policy.decimals, 24);
  assert.ok(config.cooldownSeconds >= 1);
  assert.ok(config.cooldownSeconds <= 2);
  assert.ok(config.ipLimit >= 40);
  assert.ok(config.maxAttemptsPerDay >= 1);
  if (config.ready) {
    for (const role of ["owner", "agent", "wallet", "payee", "injected", "flagged", "funder"]) {
      assert.match(config.accounts[role], /^[a-z0-9._-]+\.testnet$|^[a-f0-9]{64}$/);
    }
    assert.deepEqual(config.policy.payees, [config.accounts.payee]);
    assert.deepEqual(config.flaggedAccounts, [config.accounts.flagged]);
    assert.ok(!config.policy.payees.includes(config.accounts.injected));
    assert.ok(!config.policy.payees.includes(config.accounts.flagged));
    assert.ok(config.verifiedCodeHashes.length >= 1);
  }
});

test("environment overrides do not accept a key inside the public config", () => {
  const config = readPublicConfig();
  const next = applyEnv(config, {
    TRIAGEPAY_RPC_URL: "https://test.rpc.fastnear.com",
    TRIAGEPAY_PAYEE_ACCOUNT: "other.testnet",
    TRIAGEPAY_COOLDOWN_SECONDS: "5",
  });
  assert.equal(next.accounts.payee, "other.testnet");
  assert.equal(next.cooldownSeconds, 5);
  assert.throws(() => assertPublicConfig({ secret_key: "ed25519:abc" }));
});

test("the page script does not build HTML from strings", () => {
  const text = readFileSync(join(ROOT, "public", "app.js"), "utf8");
  for (const sink of ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval(", "new Function"]) {
    assert.equal(text.includes(sink), false, sink);
  }
});
