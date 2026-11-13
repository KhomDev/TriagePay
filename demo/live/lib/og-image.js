// PNG receipt. Satori and resvg are the same pair an OG route uses.
// @vercel/og 1.x looks for hb.wasm in the process cwd, which fails in a
// plain Node function, so this route calls the libraries itself.

import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import satori, { init as initSatori } from "satori";
import { Resvg, initWasm } from "@resvg/resvg-wasm";
import { isNearAccount } from "./accounts.js";
import { RULE_TEXT, SHARE_TAIL } from "./receipt.js";

const require = createRequire(import.meta.url);
const FONT_PATH = join(dirname(fileURLToPath(import.meta.url)), "fonts", "Geist-Regular.ttf");

const KINDS = new Set(["refused", "held", "approved", "rejected", "settled"]);
const HEADLINES = {
  refused: "REFUSED",
  held: "HELD",
  approved: "APPROVED",
  rejected: "REJECTED",
  settled: "SETTLED",
};

let ready;

function loadRenderers() {
  if (!ready) {
    ready = (async () => {
      await initSatori(await readFile(require.resolve("satori/layout.wasm")));
      await initWasm(await readFile(require.resolve("@resvg/resvg-wasm/index_bg.wasm")));
    })();
  }
  return ready;
}

function box(style, children) {
  const kids = Array.isArray(children) ? children.filter(Boolean) : children;
  return { type: "div", props: { style: { display: "flex", ...style }, children: kids } };
}

function clip(value, pattern) {
  const text = String(value || "");
  return pattern.test(text) ? text : "";
}

export function sanitizeReceiptQuery(params) {
  const kind = params.get("kind") || "";
  if (!KINDS.has(kind)) return null;
  const rule = clip(params.get("rule"), /^[A-Z0-9_]{0,48}$/);
  const payeeParam = String(params.get("payee") || "").trim().toLowerCase();
  if (payeeParam && !isNearAccount(payeeParam)) return null;
  const payeeRaw = payeeParam;
  const amount = clip(params.get("amount"), /^[0-9.]{0,48}$/);
  const tx = clip(params.get("tx"), /^[A-Za-z0-9]{0,88}$/);
  const tried = String(params.get("tried") || "")
    .replace(/[^\w\s.,:;'"()/-]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 140);
  return {
    kind,
    headline: HEADLINES[kind],
    rule: rule || kind.toUpperCase(),
    ruleText: RULE_TEXT[rule] || "The wallet applied its spend policy.",
    payee: payeeRaw,
    amount,
    tx,
    tried,
  };
}

export async function renderReceiptPng(fields) {
  await loadRenderers();
  const font = await readFile(FONT_PATH);
  const svg = await satori(
    box(
      {
        width: "100%",
        height: "100%",
        flexDirection: "column",
        background: "#07080c",
        color: "#f4f1e8",
        padding: "64px",
        fontFamily: "Geist",
      },
      [
        box({ fontSize: 22, letterSpacing: 6, color: "#e2ff54" }, "TRIAGEPAY"),
        box({ fontSize: 92, marginTop: 28, color: "#e2ff54", fontWeight: 700 }, fields.headline),
        box({ fontSize: 28, marginTop: 18, color: "#f4f1e8" }, `${fields.rule}`),
        box({ fontSize: 26, marginTop: 8, color: "#b7b3a8" }, fields.ruleText),
        fields.amount ? box({ fontSize: 26, marginTop: 28 }, `Amount  ${fields.amount}`) : null,
        fields.payee ? box({ fontSize: 26, marginTop: 8 }, `Payee  ${fields.payee}`) : null,
        box({ fontSize: 22, marginTop: 8, color: "#b7b3a8" }, fields.tried || "The agent asked the wallet to pay."),
        box({ fontSize: 20, marginTop: 36, color: "#e2ff54" }, fields.tx ? `tx ${fields.tx}` : "no transaction hash"),
        box({ fontSize: 20, marginTop: 18, color: "#8d897f" }, SHARE_TAIL),
      ],
    ),
    {
      width: 1200,
      height: 630,
      fonts: [{ name: "Geist", data: font, weight: 400, style: "normal" }],
    },
  );
  const png = new Resvg(svg, { fitTo: { mode: "width", value: 1200 } }).render().asPng();
  return png;
}

export async function renderSitePng() {
  await loadRenderers();
  const font = await readFile(FONT_PATH);
  const svg = await satori(
    box(
      {
        width: "100%",
        height: "100%",
        flexDirection: "column",
        justifyContent: "center",
        background: "#07080c",
        color: "#f4f1e8",
        padding: "72px",
        fontFamily: "Geist",
      },
      [
        box({ fontSize: 22, letterSpacing: 6, color: "#e2ff54" }, "TRIAGEPAY"),
        box({ fontSize: 64, marginTop: 28, fontWeight: 700 }, "The agent signs."),
        box({ fontSize: 64, marginTop: 8, color: "#e2ff54", fontWeight: 700 }, "The contract decides."),
        box({ fontSize: 24, marginTop: 36, color: "#b7b3a8" }, "A policy wallet on NEAR testnet and mainnet. built at NEAR HACKATHON @NEARProtocol #NEARHackathon"),
      ],
    ),
    { width: 1200, height: 630, fonts: [{ name: "Geist", data: font, weight: 400, style: "normal" }] },
  );
  return new Resvg(svg, { fitTo: { mode: "width", value: 1200 } }).render().asPng();
}
