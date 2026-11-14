// Falsara on NEAR: prices, payee, asset and endpoints. Secrets come from the
// environment only; nothing secret is in this file.

export const USDC_MAINNET = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

export function loadConfig(env = process.env) {
  const network = env.FALSARA_NETWORK === "near:testnet" ? "near:testnet" : "near:mainnet";
  const payTo = String(env.FALSARA_PAY_TO || "gateway.hlpok1lad.tg").trim();
  const asset = String(env.FALSARA_ASSET || USDC_MAINNET).trim();
  const maxTimeoutSeconds = Number(env.FALSARA_MAX_TIMEOUT_SECONDS || 900);
  const base = { network, payTo, asset, maxTimeoutSeconds };
  return {
    network,
    payTo,
    asset,
    publicBaseUrl: String(env.FALSARA_PUBLIC_BASE_URL || "https://api.falsara.online").replace(/\/$/, ""),
    coreUrl: String(env.FALSARA_CORE_URL || "").replace(/\/$/, ""),
    coreToken: String(env.FALSARA_CORE_TOKEN || ""),
    memoSecret: String(env.FALSARA_MEMO_SECRET || ""),
    endpoints: network === "near:mainnet"
      ? [env.NEAR_RPC_URL || "https://free.rpc.fastnear.com", "https://archival-rpc.mainnet.fastnear.com"]
      : [env.NEAR_RPC_URL || "https://test.rpc.fastnear.com", "https://archival-rpc.testnet.fastnear.com"],
    explorerTxBase: network === "near:mainnet" ? "https://nearblocks.io/txns/" : "https://testnet.nearblocks.io/txns/",
    routes: {
      "/v1/check-finding": {
        ...base,
        route: "/v1/check-finding",
        corePath: "/internal/near/check-finding",
        sku: "falsara.check-finding",
        label: "finding check",
        amount: String(env.FALSARA_PRICE_ATOMIC || "250000"),
        priceUsdc: "0.25",
      },
      "/v1/check-finding-poc": {
        ...base,
        route: "/v1/check-finding-poc",
        corePath: "/internal/near/check-finding-poc",
        sku: "falsara.check-finding-poc",
        label: "finding check with a Foundry proof",
        amount: String(env.FALSARA_POC_PRICE_ATOMIC || "750000"),
        priceUsdc: "0.75",
      },
    },
  };
}

export function ready(config) {
  return Boolean(config.coreUrl && config.coreToken.length >= 32 && config.memoSecret.length >= 32);
}
