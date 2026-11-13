import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { getHealth } from "../lib/demo.js";
import { MAINNET_CONTRACTS } from "../lib/mainnet.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

test("health keeps the sandbox network and names the mainnet contracts", async () => {
  const health = await getHealth();
  assert.equal(health.ok, true);
  assert.equal(health.network, "testnet");
  assert.deepEqual(health.mainnet, {
    network: "mainnet",
    factory: "triagepay.hlpok1lad.tg",
    wallet: "e2e044e80.triagepay.hlpok1lad.tg",
    agent: "tp-e2e-agent.hlpok1lad.tg",
    feeRecipient: "hlpok1lad.tg",
  });
  assert.deepEqual(health.mainnet, MAINNET_CONTRACTS);
});

test("the mainnet page badge points at the factory on NearBlocks", () => {
  const html = readFileSync(join(ROOT, "public", "app", "index.html"), "utf8");
  assert.match(html, /Live on NEAR Mainnet/);
  assert.match(html, /href="https:\/\/nearblocks\.io\/address\/triagepay\.hlpok1lad\.tg"/);
});
