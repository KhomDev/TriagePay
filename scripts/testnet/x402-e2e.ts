// Testnet end-to-end run of the x402 near-tx-proof flow (direct pay).
//
// Zero value: NEAR testnet only, throwaway implicit accounts generated for
// the run. Keys are written to <out-dir>/x402-keys/ (outside the
// repository) and never printed; evidence goes to
// <out-dir>/x402-EVIDENCE.json. Resumable: completed steps are skipped.
//
//   node scripts/testnet/x402-e2e.ts setup  --out-dir DIR
//   node scripts/testnet/x402-e2e.ts pay    --out-dir DIR --api http://127.0.0.1:PORT
//
// `setup` funds the payer from the official testnet faucet (one attempt
// per run; rerun later if it is rate-limited), creates the gateway
// account, wraps a little NEAR into wrap.testnet and registers storage.
// `pay` runs one x402 round trip against a local API started with
// APS_PROVIDER=near NEAR_NETWORK=testnet NEAR_ASSET_CONTRACT=wrap.testnet
// and NEAR_GATEWAY_ACCOUNT set to the gateway id that setup prints.

import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const clientDir = resolve(here, "..", "..", "clients", "typescript");
const requireFromClient = createRequire(join(clientDir, "package.json"));
const near: any = await import(pathToFileURL(requireFromClient.resolve("near-api-js")).href);
const { payAndScan, buildPaymentPayload, encodeHeader } = await import(pathToFileURL(join(clientDir, "src", "x402.ts")).href);
const { nearPayer } = await import(pathToFileURL(join(clientDir, "src", "payer.ts")).href);

const RPC = "https://test.rpc.fastnear.com";
const FAUCET = "https://faucet-near-testnet.onrender.com/api/faucet";
const WRAP = "wrap.testnet";
const T = (tgas: number) => BigInt(tgas) * 1_000_000_000_000n;
const NEAR = (s: string) => BigInt(near.parseNearAmount(s));

const args = process.argv.slice(2);
const command = args[0];
const flag = (name: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const outDir = flag("out-dir");
if (!outDir) throw new Error("--out-dir is required");
const keysDir = join(outDir, "x402-keys");
const evidencePath = join(outDir, "x402-EVIDENCE.json");
const provider = new near.JsonRpcProvider({ url: RPC });

type Evidence = { run: string; network: string; rpc: string; value: string; started_at: string; tooling: string; accounts: Record<string, string>; steps: any[]; stopped?: string; faucet_attempts?: any[] };
const ev: Evidence = existsSync(evidencePath)
  ? JSON.parse(readFileSync(evidencePath, "utf8"))
  : {
      run: "x402 near-tx-proof testnet e2e (direct pay)",
      network: "near:testnet",
      rpc: RPC,
      value: "zero (testnet tokens only)",
      started_at: new Date().toISOString(),
      tooling: `near-api-js 7.3.1, Node ${process.version}`,
      accounts: {},
      steps: [],
    };
const save = () => writeFileSync(evidencePath, JSON.stringify(ev, null, 2) + "\n");
const done = (step: string) => ev.steps.find((s) => s.step === step);
const record = (entry: any) => {
  ev.steps.push({ at: new Date().toISOString(), ...entry });
  save();
  console.log(JSON.stringify({ step: entry.step, tx_hash: entry.tx_hash, outcome: entry.outcome }));
};
const stop = (message: string) => {
  ev.stopped = message;
  save();
  console.log("STOP: " + message);
  process.exit(2);
};

function key(role: string): { account_id: string; secret_key: string; public_key: string } {
  mkdirSync(keysDir, { recursive: true });
  const file = join(keysDir, `${role}.json`);
  if (existsSync(file)) return JSON.parse(readFileSync(file, "utf8"));
  const pair = near.KeyPair.fromRandom("ed25519");
  const publicKey = pair.getPublicKey().toString();
  const entry = { role, account_id: near.keyToImplicitAddress(publicKey), public_key: publicKey, secret_key: pair.toString() };
  writeFileSync(file, JSON.stringify(entry, null, 2) + "\n", { mode: 0o600 });
  return entry;
}
const keyFile = (role: string) => join(keysDir, `${role}.json`);

async function viewAccount(id: string): Promise<any | null> {
  try {
    return await provider.query({ request_type: "view_account", finality: "final", account_id: id });
  } catch {
    return null;
  }
}

async function send(role: string, receiverId: string, actions: any[]) {
  const k = key(role);
  const account = new near.Account(k.account_id, provider, k.secret_key);
  const result: any = await account.signAndSendTransaction({ receiverId, actions, waitUntil: "FINAL", throwOnFailure: false });
  return { tx_hash: result.transaction.hash, outcome: result.status?.Failure ? "failure" : "success", status: result.status };
}

async function step(name: string, role: string, receiverId: string, actions: () => any[], description: string) {
  if (done(name)) return done(name);
  const result = await send(role, receiverId, actions());
  const entry = { step: name, description, signer: key(role).account_id, receiver: receiverId, tx_hash: result.tx_hash, outcome: result.outcome };
  record(entry);
  if (result.outcome !== "success") stop(`${name} failed: ${JSON.stringify(result.status).slice(0, 300)}`);
  return entry;
}

async function setup() {
  const payer = key("payer");
  const gateway = key("gateway");
  ev.accounts = { payer: payer.account_id, gateway: gateway.account_id, asset: WRAP };
  delete ev.stopped;
  save();
  let funded = await viewAccount(payer.account_id);
  if (!funded) {
    let status = 0;
    let body = "";
    try {
      const response = await fetch(FAUCET, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ accountId: payer.account_id }),
      });
      status = response.status;
      body = (await response.text()).slice(0, 400);
    } catch (error: any) {
      body = String(error?.message ?? error).slice(0, 200);
    }
    (ev.faucet_attempts ??= []).push({ at: new Date().toISOString(), faucet: FAUCET, http_status: status, response: body });
    save();
    if (status < 200 || status >= 300) stop(`faucet refused the request (HTTP ${status})`);
    for (let i = 0; i < 30 && !funded; i++) {
      funded = await viewAccount(payer.account_id);
      if (!funded) await new Promise((r) => setTimeout(r, 4000));
    }
    if (!funded) stop("payer account not visible after the faucet call");
  }
  console.log("payer balance NEAR " + near.formatNearAmount(funded.amount, 4));
  await step("create_gateway", "payer", gateway.account_id, () => [near.actions.transfer(NEAR("0.05"))], "create the gateway implicit account with 0.05 NEAR");
  await step("wrap_and_register", "payer", WRAP, () => [
    near.actions.functionCall("storage_deposit", {}, T(30), NEAR("0.00125")),
    near.actions.functionCall("storage_deposit", { account_id: gateway.account_id }, T(30), NEAR("0.00125")),
    near.actions.functionCall("near_deposit", {}, T(30), NEAR("0.01")),
  ], "register storage for payer and gateway on wrap.testnet and wrap 0.01 NEAR");
  console.log("GATEWAY=" + gateway.account_id);
}

async function pay() {
  const api = flag("api");
  if (!api) throw new Error("--api is required");
  if (done("x402_round_trip")) {
    console.log("already done");
    return;
  }
  const envelope = {
    title: "Rounding drain in vault share accounting",
    claimed_impact: "Permanent loss of user funds via inflated share ratio",
    mechanism: "First depositor inflates the share price before the pool opens",
    attacker_capability: "Any external address with collateral",
    preconditions: "Vault freshly deployed with empty reserves",
    component_class: "vault",
    scope_statement: "Vault deposit and mint path is in scope for this review",
    deployed_state_evidence: "Vault live at 0x1234567890abcdef since block 5100",
    public_rules_snapshot: "https://archive.example.org/rules/v1",
    known_issue_declaration: null,
    reproduction_summary: "Step 1: mint one share. Step 2: donate collateral. Step 3: redeem.",
    evidence_refs: ["https://example.test/issue-family-1"],
    code_excerpt: "function mint() external returns (uint256) { ... }",
  };
  const started = Date.now();
  let requirement: any = null;
  const payer = nearPayer({ mode: "direct", keyFile: keyFile("payer"), rpcUrl: RPC, log: (l: string) => console.log(l) });
  const result = await payAndScan(api, envelope, async (r: any) => {
    requirement = r;
    if (r.network !== "near:testnet") throw new Error("refusing a non-testnet requirement");
    return payer(r);
  }, { network: "near:testnet", maxPendingRetries: 60 });
  record({
    step: "x402_round_trip",
    description: "POST /v2/scan -> 402 -> direct ft_transfer on wrap.testnet -> retry with PAYMENT-SIGNATURE -> 200",
    requirement,
    tx_hash: result.proof.tx_hash,
    signer: result.proof.signer_id,
    outcome: result.paymentResponse.success ? "success" : "failure",
    payment_response: result.paymentResponse,
    attempts: result.attempts,
    report_decision: result.body?.report?.decision,
    settlement: result.body?.settlement,
    elapsed_ms: Date.now() - started,
  });
  // Replay the same proof and request: the stored result, no new charge.
  const signature = encodeHeader(buildPaymentPayload(requirement, result.proof, { url: "/v2/scan" }));
  const replay = await fetch(api.replace(/\/+$/, "") + "/v2/scan", {
    method: "POST",
    headers: { "content-type": "application/json", "PAYMENT-SIGNATURE": signature },
    body: JSON.stringify(envelope),
  });
  const replayBody = await replay.json();
  const balances = await (await fetch(api.replace(/\/+$/, "") + "/accounts/x402:dev-agent")).json();
  record({
    step: "replay_same_proof",
    description: "the same PAYMENT-SIGNATURE and request again",
    tx_hash: result.proof.tx_hash,
    outcome: replay.status === 200 && JSON.stringify(replayBody) === JSON.stringify(result.body) ? "identical_response" : `unexpected_${replay.status}`,
    balances_after: balances,
  });
  console.log("PAID_TX=" + result.proof.tx_hash);
}

if (command === "setup") await setup();
else if (command === "pay") await pay();
else throw new Error("usage: x402-e2e.ts setup|pay --out-dir DIR [--api URL]");
