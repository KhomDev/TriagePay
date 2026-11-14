# Falsara on NEAR mainnet

Falsara is a paid pre-submission check for one candidate security finding. It runs the finding through pinned public gates ([viability-gate](https://github.com/KhomDev/viability-gate) and, with a Foundry proof, [realism-gate](https://github.com/KhomDev/realism-gate)) and answers `REFUSE`, `NO_REFUSAL` or `INCOMPLETE`, quoting each gate that refused. It existed before NEAR HACKATHON and took payment only on Base.

What was built at NEAR HACKATHON is its NEAR integration: an x402 gateway that takes USDC on NEAR mainnet and verifies each payment from the USDC contract's own event, and the TriagePay agent that pays it through a policy wallet. That is the part to judge (rules Q-002 and Q-015).

## Try it

| Do | See |
|---|---|
| Open https://triagepay-demo.vercel.app/ and press **Ask Falsara on mainnet** in step 3 | The agent gets a 402 from Falsara, pays 0.25 USDC through its policy wallet, and shows Falsara's verdict with the quoted gate reasons and the NearBlocks link. The wallet's cap allows six checks a day |
| `curl -i -X POST https://api.falsara.online/v1/check-finding -H 'content-type: application/json' -d @finding.json` | `402` with a `PAYMENT-REQUIRED` header: `near:mainnet`, native USDC, 250000 atomic, payee `gateway.hlpok1lad.tg`, a memo bound to this finding |
| The same request with a finding the gates cannot decide | `422 INCOMPLETE_NOT_CHARGED` with the missing evidence, and no payment requirement |
| Open https://api.falsara.online/ | Falsara's page with live status |

`falsara-near.vercel.app` is the same gateway under its Vercel name; earlier evidence files name it.

## How it runs

| Part | Where | Code |
|---|---|---|
| NEAR x402 gateway | Vercel project `falsara-near`, public | [`services/falsara-near`](../../../services/falsara-near) in this repository |
| Falsara core: request schemas, the two pinned gate binaries (static Linux builds), the verdict mapping | Vercel project `falsara-core`, answers only the gateway's internal token | Falsara's private repository; the serverless packaging was added in its `near` branch and built by its CI |
| The paying agent | The TriagePay demo, `POST /api/falsara` | [`demo/live/lib/falsara.js`](../../../demo/live/lib/falsara.js) |

The gateway keeps no state. The memo in each requirement is an HMAC over the route, the expiry, a nonce, the hash of the exact finding, the price, the payee, the asset and the network. A payment whose memo verifies for this body pays for this body; the same proof and body return the same verdict again; the same proof with another finding is refused (`proof_already_consumed`). A payment counts only when the transaction is final, the receipt was executed by the USDC contract itself and succeeded, its NEP-141 `ft_transfer` event pays the payee with that memo and exactly the price, and its block is not later than the expiry. These are the rules of TriagePay's Rust verifier (`crates/near-adapter/src/verify.rs`), ported to JavaScript for a serverless host; the tests run them against the real mainnet receipts of an earlier payment.

Before it asks for money the gateway runs the gates once. A finding they cannot decide is answered `422 INCOMPLETE_NOT_CHARGED`, so on NEAR, as on Base, only decisions are paid for.

## Mainnet record, 2026-11-14

All amounts are native USDC (`17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`), 6 decimals.

| Step | Transaction |
|---|---|
| The owner funds the factory-created wallet `e2e044e80.triagepay.hlpok1lad.tg` with 1.50 USDC | [DovzA8Jb](https://nearblocks.io/txns/DovzA8JbY9DaPtjSVEJnag6CB512Mwvns2miwP24RQXP) |
| The owner sets the agent's USDC caps: 0.25 per call, 1.50 per UTC day | [CJoUnumd](https://nearblocks.io/txns/CJoUnumdCYJcjit9gTEdp8jrdMLsDRMsDFWv8mR8pPK2) |
| The owner adds a pay-only function-call key for the hosted demo (receiver: the wallet, method: `pay`) | [26zT6eNF](https://nearblocks.io/txns/26zT6eNF67bxvhcu5LPYzhf5R28Z4sQf3VRfU6DBd4qx) |
| The agent buys a check from a local run: 0.25 USDC, verdict `REFUSE` | [49Jhqhsz](https://nearblocks.io/txns/49JhqhszsNUdiny8EStKEWD1bWsfc3eNYUFSCgw4Qz9R) |
| The agent buys a check from the hosted demo: 0.25 USDC, verdict `REFUSE` | [AZ9kPAz8](https://nearblocks.io/txns/AZ9kPAz84NGpyqhNjibBHKNtFGthMwH58hHiV4Pgr74U) |

The finding both checks bought is "Owner can withdraw all user deposits". Falsara refused it, quoting viability-gate rule P2 ("Privileged-role-gated against a program exclusion", category `G2_exclusion_clause`) and P25 ("Impact missing or not quantified"). That is the product doing its job: a privileged-role finding is what a bounty program's exclusion clause rejects.

Full records: [EVIDENCE.json](2026-11-14/EVIDENCE.json) and [budget.json](2026-11-14/budget.json).

## Limits

- The Falsara core is private code; the gates it runs are public and pinned. Its serverless build cannot clone a target repository (no `git` in the function), so evidence packs that name a target checkout come back `INCOMPLETE` there.
- The live demo button checks one fixed sample finding, so a visitor cannot spend the wallet's budget on arbitrary text. The API takes any finding.
- The wallet's beta deposit ceiling is 100 USDC and the contracts are unaudited.
