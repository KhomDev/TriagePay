#!/usr/bin/env node
// aps-x402 CLI.
//
//   pay   read a payment requirement (JSON) on stdin, pay it, print
//         {"tx_hash","signer_id"} as the last stdout line. This is the
//         contract clients/python's shell_payer expects.
//   scan  the full x402 round trip: request /v2/scan, pay, retry, print
//         the decoded PAYMENT-RESPONSE and the report decision.
//
// Options: --mode direct|wallet  --key-file PATH  [--account ID]
//          [--wallet ID] [--rpc-url URL] [--allow-mainnet]
//          scan only: --url BASE --envelope FILE [--network near:testnet]
// Progress goes to stderr; secrets are never printed.

import { readFileSync } from "node:fs";
import { assertNetworkAllowed, type Mode } from "./actions.ts";
import { payAndScan, type Requirement } from "./x402.ts";

export function parseArgs(argv: string[]): { command: string; flags: Record<string, string | boolean> } {
  const [command = "", ...rest] = argv;
  const flags: Record<string, string | boolean> = {};
  for (let i = 0; i < rest.length; i++) {
    const arg = rest[i];
    if (!arg.startsWith("--")) throw new Error(`unexpected argument ${arg}`);
    const name = arg.slice(2);
    if (name === "allow-mainnet") {
      flags[name] = true;
      continue;
    }
    const value = rest[i + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`--${name} needs a value`);
    flags[name] = value;
    i++;
  }
  return { command, flags };
}

function mode(flags: Record<string, string | boolean>): Mode {
  const value = flags.mode ?? "direct";
  if (value !== "direct" && value !== "wallet") throw new Error("--mode must be direct or wallet");
  return value;
}

async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

const log = (line: string) => process.stderr.write(line + "\n");

export async function main(argv: string[]): Promise<number> {
  const { command, flags } = parseArgs(argv);
  const str = (name: string) => (typeof flags[name] === "string" ? (flags[name] as string) : undefined);
  const allowMainnet = flags["allow-mainnet"] === true;
  if (command === "pay") {
    const requirement = JSON.parse(await readStdin()) as Requirement;
    assertNetworkAllowed(requirement.network, allowMainnet);
    const keyFile = str("key-file");
    if (!keyFile) throw new Error("--key-file is required");
    const { sendPayment } = await import("./payer.ts");
    const proof = await sendPayment(requirement, {
      mode: mode(flags),
      keyFile,
      account: str("account"),
      wallet: str("wallet"),
      rpcUrl: str("rpc-url"),
      log,
    });
    process.stdout.write(JSON.stringify(proof) + "\n");
    return 0;
  }
  if (command === "scan") {
    const url = str("url");
    const envelopePath = str("envelope");
    const keyFile = str("key-file");
    if (!url || !envelopePath || !keyFile) throw new Error("scan needs --url, --envelope and --key-file");
    const envelope = JSON.parse(readFileSync(envelopePath, "utf8"));
    const { nearPayer } = await import("./payer.ts");
    const pay = nearPayer({ mode: mode(flags), keyFile, account: str("account"), wallet: str("wallet"), rpcUrl: str("rpc-url"), log });
    const result = await payAndScan(url, envelope, async (requirement) => {
      assertNetworkAllowed(requirement.network, allowMainnet);
      return pay(requirement);
    }, { network: str("network") });
    process.stdout.write(JSON.stringify({ paymentResponse: result.paymentResponse, decision: result.body?.report?.decision, attempts: result.attempts }) + "\n");
    return 0;
  }
  throw new Error("usage: cli.ts pay|scan [options] (see the file header)");
}

const invoked = process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/").split("/").pop() ?? "");
if (invoked) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (error) => {
      log(`aps-x402: ${error?.message ?? error}`);
      process.exit(1);
    },
  );
}
