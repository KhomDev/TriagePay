# TriagePay

[![Mainnet watch](https://github.com/KhomDev/TriagePay/actions/workflows/mainnet-watch.yml/badge.svg)](https://github.com/KhomDev/TriagePay/actions/workflows/mainnet-watch.yml)

TriagePay is a spending wallet for AI agents on NEAR. It is a smart contract that holds the agent's money and decides every payment the agent asks for, so an agent that is tricked, buggy or stolen cannot pay anyone or anything its owner did not allow.

**What it is, exactly**

- **The policy wallet** (`contracts/policy-wallet`): a NEAR contract that holds NEP-141 tokens (USDC, wNEAR) for one owner. The agent gets a function-call key that can call only `pay` on it. Every `pay` is checked on chain against the owner's payee allowlist, a per-call cap and a per-day cap; a payment above a threshold waits in an on-chain queue until the owner approves or rejects it. The owner can pause the wallet or revoke the agent at any time. The wallet itself has no access keys and no upgrade path.
- **The wallet factory** (`contracts/wallet-factory`, live as `triagepay.hlpok1lad.tg`): deploys one wallet per owner on mainnet.
- **x402 payments with on-chain proof** (`crates/`): a server can charge an agent per HTTP call with x402 (`402 Payment Required`, pay, retry). It accepts the payment only after reading the token contract's own NEP-141 transfer event, with its memo, from a finalized receipt.
- **A real service the agent pays: Falsara on NEAR** (`services/falsara-near`, live at https://api.falsara.online): Falsara checks a security finding against pinned public gates before it is submitted, for 0.25 USDC per check on NEAR mainnet. The agent buys checks through its policy wallet ([AZ9kPAz8](https://nearblocks.io/txns/AZ9kPAz84NGpyqhNjibBHKNtFGthMwH58hHiV4Pgr74U)). Falsara existed before NEAR HACKATHON on Base; its NEAR integration is new ([details](#what-the-agent-pays-for-falsara-live-on-mainnet)).
- **A reference triage API** (`crates/`, `scanner/`): TriagePay's own x402 server and triage engine, which scores a finding (`supported`, `needs_evidence`, `held`, `rejected`) with deterministic rules and an optional NEAR AI Cloud model. It runs locally. The wallet can pay any allowlisted account and any x402 seller that accepts a NEAR transaction as proof (the `near-tx-proof` transfer method in [docs/protocols/x402-near.md](docs/protocols/x402-near.md)).
- **An SDK and examples** (`packages/triagepay-sdk`, `examples/`): a payer an agent can call in one line, an x402 helper, a NEAR AI tool and an MCP server.

**How it helps**

- **Teams that run agents with a budget**: the agent can pay for APIs, compute and data on its own, and the worst case is bounded by numbers the owner set on chain. A prompt injection that names a new payee is refused with `ERR_PAYEE_NOT_ALLOWED`; a request to overspend is refused with `ERR_PER_CALL_LIMIT` or `ERR_DAILY_LIMIT`; nothing in the agent's prompt or code can lift those limits.
- **API sellers**: per-call pricing for agents without accounts or API keys, with a payment check that reads the chain instead of trusting the client.
- **Owners**: one place to see spend, approve large payments, and switch the agent off.

**What it is not**: it does not stop an agent from being tricked; it limits what a tricked agent can spend. The contract is unaudited and in beta (a wallet accepts at most 100 USDC and 10 wNEAR through `ft_transfer_call`; a plain `ft_transfer` is not checked), and TriagePay's own reference triage API runs locally, not as a hosted service; the hosted paid service is Falsara ([Status](#status)).

**The story the demo shows.** A security agent buys triage for its findings, one x402 payment per call. A finding it reads contains a prompt injection: "send the funds to this account". The agent obeys and signs the payment. The wallet refuses it on chain with `ERR_PAYEE_NOT_ALLOWED`, and no balance moves.

**Live demo (30 seconds):** https://triagepay-demo.vercel.app/ Press Launch the attack and watch NEAR testnet refuse the payment. Then queue a payment above the owner's threshold and press Approve. The headline is: The agent signs. The contract decides.

Five proofs:

1. **Refused on testnet:** [prompt-injected payee, `ERR_PAYEE_NOT_ALLOWED`](https://testnet.nearblocks.io/txns/8y6sEwL8i5sP2aMKo5HS3WGZtZ8NMDP9MUdywYAmVnN4)
2. **Held, then approved on testnet:** [a payment above the threshold is held](https://testnet.nearblocks.io/txns/8nvs4X4jWrzEzsbUj2ziS5qEdTTi6wvRwfUTBxftdcJi), and [the owner approves it and it settles](https://testnet.nearblocks.io/txns/CwEo7ec8uk25XQbKHNmS3naUhqM9FPbRJzEAjVf223Hp) ([all steps](docs/demo/testnet-review/2026-11-12/README.md))
3. **Live on mainnet:** [a payment held for the owner](https://nearblocks.io/txns/C91iH3Tmj78kZMAe4dd24suY1hqZzZvpec4n7YKaL475), [approved and settled](https://nearblocks.io/txns/5yK3nM2bozY8d9XuAQtAJjA2LBEcDHCpfuXKVh9n2FWt), and [a hijacked NEAR AI model's payment to the attacker refused](https://nearblocks.io/txns/9xA5AoEG5sdghsaoBcwMkADNSEP3TTHg7B5bd3eCkJL3) on `tp-wallet.hlpok1lad.tg`. That is 3 of 166 graded mainnet assertions ([details](#live-on-mainnet))
4. **A real product bought on mainnet:** the agent pays [Falsara](https://api.falsara.online/) 0.25 USDC through its policy wallet for one finding check ([AZ9kPAz8](https://nearblocks.io/txns/AZ9kPAz84NGpyqhNjibBHKNtFGthMwH58hHiV4Pgr74U)); Falsara reads the USDC contract's transfer event from the final receipt and answers with its verdict ([evidence](docs/demo/falsara-near/README.md))
5. **x402 paid on mainnet:** [a factory-created wallet pays 0.001 USDC for one scan](https://nearblocks.io/txns/CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG); the server reads the USDC contract's transfer event and answers 200, and the same proof cannot buy a second scan ([evidence](docs/demo/mainnet-x402/2026-11-14/README.md))

This project was built at NEAR HACKATHON, in the `security` and `agents` tracks. #NEARHackathon @NEARProtocol

## Screenshots

Live demo: [triagepay-demo.vercel.app](https://triagepay-demo.vercel.app). Screenshots captured 14 Nov 2026 (Europe/Budapest) from the live demo. HEAD `1df52d8` tightens Mainnet-label typography and brings the `/app` form on screen; those UI fixes are in this repository but were not yet deployed to the live demo when the screenshots were taken, so the live page may still look slightly older.

<p align="center"><img src="docs/screenshots/home-attack-refused.png" alt="TriagePay home: the agent signs a prompt-injected payment and the contract refuses it on testnet with ERR_PAYEE_NOT_ALLOWED" width="900"></p>

The live demo after pressing **Launch the attack**. The agent signs a payment to an injected payee; the policy wallet refuses it on NEAR testnet with `ERR_PAYEE_NOT_ALLOWED`.

<p align="center"><img src="docs/screenshots/live-counts-falsara.png" alt="Live counts, Falsara on mainnet, and the on-chain refusal receipt" width="900"></p>

Live counts from NearBlocks, the Falsara mainnet check, and the on-chain refusal receipt.

<p align="center"><img src="docs/screenshots/drain-me.png" alt="Drain Me playground with spend stats and visitor injection attempts" width="900"></p>

**Drain Me**: visitors can write their own injection. The contract still stops the payment (14 attempts, 14 refusals, 0 drained in this capture).

<p align="center"><img src="docs/screenshots/agent-passport.png" alt="Agent passport showing pay-only key caps, allowlist, and recent on-chain pays and refusals" width="900"></p>

Agent passport for `tpd-e947e7-agent.testnet`: pay-only key, caps, allowlist, and recent on-chain pays and refusals.

<p align="center"><img src="docs/screenshots/home-mobile.png" alt="TriagePay mobile home" width="390"></p>

Mobile home (390×844).

## Promo video

<!-- PROMO_VIDEO_SLOT: keep this heading. Replace the placeholder line below with a 16:9 embed (or a link to the file) when the final render is ready. The 9:16 cut is for the X submission post. Do not delete these markers. -->

_Official 16:9 demo video will be embedded here._

<!-- /PROMO_VIDEO_SLOT -->



## What the agent pays for: Falsara, live on mainnet

[Falsara](https://api.falsara.online/) is a paid pre-submission check for one candidate security finding. It runs the finding through pinned public gates ([viability-gate](https://github.com/KhomDev/viability-gate), and [realism-gate](https://github.com/KhomDev/realism-gate) for a Foundry proof) and answers `REFUSE`, `NO_REFUSAL` or `INCOMPLETE`, quoting each gate that refused. A bug-hunting agent produces many candidate findings; Falsara drops the ones a bounty program would reject before a person reads them.

| Do | See |
|---|---|
| On https://triagepay-demo.vercel.app/, step 3, press **Ask Falsara on mainnet** | The agent gets a 402 from Falsara (0.25 USDC on `near:mainnet`), pays through its policy wallet, and shows the verdict with the quoted gate reasons and the NearBlocks link. The wallet's cap (0.25 USDC per call, 1.50 per day) is what limits this button |
| `curl -i -X POST https://api.falsara.online/v1/check-finding -H 'content-type: application/json' -d '{"schema_version":"1.0.0","title":"Owner can withdraw all user deposits","severity":"high","protocol":"ExampleVault","contract":"src/Vault.sol","impact":"The owner calls emergencyWithdraw and moves every user deposit to an address the owner picks.","preconditions":"The caller holds the owner role, which only the deployer has."}'` | `402` and a `PAYMENT-REQUIRED` header: native USDC, 250000 atomic, payee `gateway.hlpok1lad.tg`, a memo bound to this finding |
| The same call with a finding the gates cannot decide | `422 INCOMPLETE_NOT_CHARGED` with the missing evidence, and no payment asked |

How payment is checked: the memo is an HMAC over the exact finding, the price, the payee and the expiry, so the gateway keeps no state. A payment counts only when the USDC contract's own receipt in a final transaction has an `ft_transfer` event that pays the payee with that memo and exactly the price, before the expiry; the same proof returns the same verdict again and cannot buy a check of another finding. These are the rules of TriagePay's Rust verifier, ported to the serverless gateway and tested against real mainnet receipts. Before it asks for money the gateway runs the gates once, so a finding they cannot decide is never charged.

Falsara existed before NEAR HACKATHON and took payment on Base only. What was built here is the NEAR integration: the gateway in [`services/falsara-near`](services/falsara-near), the serverless core it calls, and the agent that pays it ([`demo/live/lib/falsara.js`](demo/live/lib/falsara.js)). The core runs Falsara's existing schemas, pinned gates and verdict mapping as a private Vercel project that answers only the gateway. Mainnet record and architecture: [docs/demo/falsara-near/README.md](docs/demo/falsara-near/README.md).

## The reference triage API (local)

TriagePay's own paid service is real and runs locally. An agent sends one security finding (title, component, claimed impact, evidence, the bounty program's rules) to `POST /v2/scan` and gets back a verdict with reasons:

| Verdict | Meaning |
|---|---|
| `rejected` | A deterministic gate failed; the reasons name the category |
| `held` | A gate flagged something a person should look at first |
| `needs_evidence` | The claim may be real, but the evidence is missing or only a placeholder |
| `supported` | No deterministic objection |

Two stages produce it:

- **Deterministic scanner (Stage A).** Nine gates, one per public finding category, compiled from a versioned ruleset. The same finding always gets the same verdict. Text inside a finding that tries to instruct the reader is flagged and never changes the outcome.
- **NEAR AI judge (Stage B, optional).** A model on NEAR AI Cloud gives a second opinion with a short rationale. It is advisory: it can lower a verdict, never raise it.

Why an agent would pay for this per call: a bug-hunting or code-review agent produces many candidate findings, and most should not reach a bounty program or a human reviewer. The triage call filters them first and says why. With x402 there is no account or API key to set up: the 402 answer names the asset, the payee, the amount and a memo, and the agent pays and retries. That payment is exactly the step a prompt injection goes after, which is why the agent pays through the policy wallet.

Internals (rule evaluators, the Stage B prompt and schema, the combiner, the knowledge compiler that builds the ruleset): [docs/engine/overview.md](docs/engine/overview.md).

## Why TriagePay and not another agent spend guard

These are the facts I can point to. I have not benchmarked other tools.

- **The limits are on chain, not in the agent.** The agent holds a NEAR function-call key that can call only `pay` on the wallet. Caps, the payee allowlist, pause and revoke live in the contract, so a prompt-injected or stolen key cannot switch them off. Owner methods need 1 yoctoNEAR, which no function-call key can attach.
- **The server checks the payment from the token contract's own events.** An x402 payment counts only when the token contract's own NEP-141 `ft_transfer` event, in a finalized receipt, pays the gateway with the server-issued memo. Logs from any other contract are ignored. This ran on testnet and, on 2026-11-14, on mainnet in native USDC through a factory-created wallet ([CwLaJmaX](https://nearblocks.io/txns/CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG), [evidence](docs/demo/mainnet-x402/2026-11-14/README.md)) ([how](#how-the-server-knows-it-was-paid)).
- **An owner review queue on chain.** A new payee or an amount above a threshold is held in the contract until the owner approves or rejects it. Run on testnet: [held](https://testnet.nearblocks.io/txns/8nvs4X4jWrzEzsbUj2ziS5qEdTTi6wvRwfUTBxftdcJi), [approved](https://testnet.nearblocks.io/txns/CwEo7ec8uk25XQbKHNmS3naUhqM9FPbRJzEAjVf223Hp), [rejected](https://testnet.nearblocks.io/txns/CcUmBvhQPBvtPJ9pg4NF225U4XfaJEmwjLEZV9gCAaWR).
- **Live on mainnet** with real (tiny) wNEAR amounts: the review-queue wallet, 166 graded on-chain assertions, three rounds of NEAR AI models as the agent, and the paid, refused, and revoked proofs ([below](#live-on-mainnet)).
- **A reusable SDK.** `packages/triagepay-sdk` is a one-line policy-wallet payer, an x402 helper and a NEAR AI tool (13 tests). The two-minute setup, the MCP server, and an outside agent are in [examples](examples/README.md).
- **A claims audit.** Every testable claim in the docs is mapped to its evidence, with what is limited or open said plainly: [docs/claims-audit.md](docs/claims-audit.md).

## Live on mainnet

I deployed the policy wallet on NEAR mainnet and ran it with real (tiny) wNEAR amounts on 2026-11-12. On 2026-11-13 I closed that first wallet and redeployed the review-queue build at the same account. It is still live, built from `ef7472c` (code hash `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC`). The 2026-11-12 transactions below still resolve.

- Wallet contract: [`tp-wallet.hlpok1lad.tg`](https://nearblocks.io/address/tp-wallet.hlpok1lad.tg).
  - Code hash `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC`, built from `ef7472c`.
  - 0 access keys. Owner `hlpok1lad.tg`. Token `wrap.near`.
  - Payee allowlist `[gateway.hlpok1lad.tg]`. 1,000,000 per call and 4,000,000 per UTC day, in yocto-wNEAR.
  - Owner review is on: a payment above 500,000 to the allowlisted payee is held for the owner. A payee off the allowlist is refused outright: the owner switched unknown-payee holds off on 2026-11-13 at 05:12 UTC ([EBhKfGSa](https://nearblocks.io/txns/EBhKfGSakUdJWPV9jKzjZHzoGGWxC4WgQKM3tp4CR5YT)). The campaign below ran before that change, with unknown payees held.
- Agent: [`tp-agent2.hlpok1lad.tg`](https://nearblocks.io/address/tp-agent2.hlpok1lad.tg), active. Its only key is a function-call key for `pay` on the wallet.
- **Paid:** [allowlisted payee, inside the cap, `pay_settled`](https://nearblocks.io/txns/Hh6BquChhVfHMkapJKuG4AXv3Nbe7u2ZTV9xLezoCR2C)
- **Refused:** [prompt-injected payee, `ERR_PAYEE_NOT_ALLOWED`](https://nearblocks.io/txns/8JBgqcYGjGjkBDoyCehPnLYZRTU58PYnVzWNBccw2yJi). [Over the daily cap, `ERR_DAILY_LIMIT`](https://nearblocks.io/txns/45hcfFNUHujLFcT8L6YECoZT8CLFjScRpBY5kuRfgR4q). [A non-owner calls `withdraw`, `ERR_NOT_OWNER`](https://nearblocks.io/txns/4knb47ejw7unkh3o57PkaaT6p6LDApHznNXKrap8YmCw)
- **Owner review:** [held](https://nearblocks.io/txns/C91iH3Tmj78kZMAe4dd24suY1hqZzZvpec4n7YKaL475). [The owner approves and it settles](https://nearblocks.io/txns/5yK3nM2bozY8d9XuAQtAJjA2LBEcDHCpfuXKVh9n2FWt). [A second one is rejected](https://nearblocks.io/txns/8vU2BbPSKze5GL4sRLPBqg3SsQPGTvpA8n7RWXcHNdt6)
- **A hijacked model:** `Qwen3.8-27B-abliterated` on NEAR AI Cloud, told to obey payment instructions in the finding, sent five "tips" to the attacker. The wallet refused [all](https://nearblocks.io/txns/9xA5AoEG5sdghsaoBcwMkADNSEP3TTHg7B5bd3eCkJL3) [five](https://nearblocks.io/txns/E2w4zp21Pu4NjYMk6jr6aNkFotr5kxRACRzvEiQti1hs)

The campaign covered:

- 166 graded on-chain assertions: limits, zero and overflow amounts, malformed args, everything the pay-only key cannot do, a real non-owner account, revoke and restore, pause, the review queue, and three rounds of LLM agents. All 166 held. The first report counted 170; the [re-grade](docs/demo/mainnet-campaign/2026-11-13/REGRADE.md) drops 4 model rounds where the model never tried to pay.
- 183 transactions, 0.066 NEAR of gas, and about USD 0.046 of NEAR AI.
- One design issue: an agent can fill the owner's review queue and reserve the day with junk holds. Its own legitimate payments then fail until the owner rejects them. Funds stay safe; the cost is availability. This applies to `tp-wallet`'s build (`ef7472c`). From `94e62f1` on, a hold is checked and not reserved, so junk holds take queue slots (3 open per agent, 8 per wallet, 24-hour expiry) but not the day's budget.

Everything is in [`docs/demo/mainnet-campaign/2026-11-13/`](docs/demo/mainnet-campaign/2026-11-13/README.md), and the script is `scripts/mainnet/campaign.mjs`. The first mainnet run of 2026-11-12 (the build before the review queue) is in [`docs/demo/mainnet-e2e/2026-11-12/`](docs/demo/mainnet-e2e/2026-11-12/README.md). `wrap.near` emits no NEP-141 events, so the x402 receipt check cannot run on this wallet. It ran on mainnet on 2026-11-14 in native USDC instead: the factory-created wallet paid 0.001 USDC per scan, the server matched the USDC contract's own `ft_transfer` event and answered 200, the same proof replayed the stored response, and reusing it for another finding was refused ([CwLaJmaX](https://nearblocks.io/txns/CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG), [evidence](docs/demo/mainnet-x402/2026-11-14/README.md)).

I re-check the live deployment every six hours, and I can start the same check by hand. The workflow is read-only: public RPC at `https://free.rpc.fastnear.com` and `https://test.rpc.fastnear.com`, no key and no signature. It confirms `tp-wallet.hlpok1lad.tg` exists, its code hash is `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC`, it has no access keys, and its owner, pause flag, token, agent caps, allowlist, review threshold, and queue capacity match [`scripts/mainnet/expected-live.json`](scripts/mainnet/expected-live.json). It confirms `tp-agent2.hlpok1lad.tg` is active and its only key is `ed25519:69i3HGAR51MzFSvGLjQqnZhk6Dpo39VGnaNU9LJscCdn`, a function-call key limited to `pay` on that wallet. It does not treat today's spend or the key's remaining gas allowance as fixed numbers. It confirms the four recorded proof transactions still resolve: `2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ` paid, `HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F` refused, `CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh` revoke, `C1BnCCrEMYfxhirf283GfGr2sfeBMaZ88fimxQwoKt2d` inactive. It confirms `tpd-e947e7-wallet.testnet` still answers with the recorded owner, token, caps, and code hash, and that `https://triagepay-demo.vercel.app/api/health` reports ok. A new mainnet wallet is another entry in that JSON.

## Live on NEAR Mainnet

The sandbox on the homepage stays on testnet. These are the mainnet accounts. There are two mainnet wallets: the campaign wallet, deployed by hand, and a wallet the factory created.

| Role | Account |
|---|---|
| Factory | [`triagepay.hlpok1lad.tg`](https://nearblocks.io/address/triagepay.hlpok1lad.tg) |
| Fee recipient | [`hlpok1lad.tg`](https://nearblocks.io/address/hlpok1lad.tg) |
| Factory-created wallet (the page's Mainnet toggle) | [`e2e044e80.triagepay.hlpok1lad.tg`](https://nearblocks.io/address/e2e044e80.triagepay.hlpok1lad.tg) |
| Its agent | [`tp-e2e-agent.hlpok1lad.tg`](https://nearblocks.io/address/tp-e2e-agent.hlpok1lad.tg) |
| Campaign wallet | [`tp-wallet.hlpok1lad.tg`](https://nearblocks.io/address/tp-wallet.hlpok1lad.tg) |
| Its agent | [`tp-agent2.hlpok1lad.tg`](https://nearblocks.io/address/tp-agent2.hlpok1lad.tg) |

`/api/health` reports the sandbox `network` and a `mainnet` object with the factory, the factory-created wallet, its agent and the fee recipient. The six-hour watch checks the campaign wallet.

Sample transactions. I copy a hash only when it is already in a committed evidence file.

| What | Hash | Where I read it |
|---|---|---|
| Factory deploy | [`Fh7252ctZhBJYErMk7FhB9yGHGVnqPfJNFwy84fLGWyp`](https://nearblocks.io/txns/Fh7252ctZhBJYErMk7FhB9yGHGVnqPfJNFwy84fLGWyp) | Deployed from `main` at `1b1a578`. On-chain code hash `9d1be4SWuAXqDvJZ2RpazjPEhjMZGHwd5omS7b2hBAhs` |
| Allowlisted pay on the demo wallet | [`2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ`](https://nearblocks.io/txns/2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ) | `docs/demo/mainnet-e2e/2026-11-12/mainnet-EVIDENCE.json` |
| Refused payee | [`HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F`](https://nearblocks.io/txns/HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F) | same file |
| Owner revokes the agent | [`CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh`](https://nearblocks.io/txns/CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh) | same file |
| Revoked agent is refused | [`C1BnCCrEMYfxhirf283GfGr2sfeBMaZ88fimxQwoKt2d`](https://nearblocks.io/txns/C1BnCCrEMYfxhirf283GfGr2sfeBMaZ88fimxQwoKt2d) | same file |

## Wallet factory

The factory is `triagepay.hlpok1lad.tg`. I deployed this build from `main` at `1b1a578` ([deploy transaction](https://nearblocks.io/txns/Fh7252ctZhBJYErMk7FhB9yGHGVnqPfJNFwy84fLGWyp)). Its on-chain code hash is `9d1be4SWuAXqDvJZ2RpazjPEhjMZGHwd5omS7b2hBAhs`. Each wallet gets its own copy of the policy-wallet wasm. I do not use a global contract; that path is behind the `global-contracts` cargo feature and it is off.

`create_wallet` takes a 1 NEAR fee plus 2.52884 NEAR of refundable storage. The storage is the wasm (221,884 bytes after `wasm-opt -Oz`, 2.21884 NEAR), 0.01000 NEAR of state, and 0.30000 NEAR left liquid so a few holds can be stored. Excess comes back immediately. The fee moves to `hlpok1lad.tg` only after `new` succeeds. A failed create refunds the fee and the storage. The factory stores the wasm compressed and inflates it in `create_wallet`, so the factory account does not lock a second full copy. The bytes the deployed factory puts on each wallet are the `wasm-opt` output from this host, sha256 `17acbefc65151683d5a37771b785ec0e5a9fa6debfbdc566084370a714e25b7e`. The source at HEAD embeds a rebuilt wallet with the 2026-11-14 fixes (219,653 bytes, sha256 `cb4f94697d30bf18b3f887694ec78b9eeb6992a508e02ea4c3cb39b212fc5974`), and its `create_wallet` refuses to deploy bytes that do not match. Only a new factory account would serve it. `new` accepts only the new account or its parent, so a two-step deploy cannot be front-run. The build remaps the `index.crates.io-<hash>` registry folder last. GitHub's rebuild is the same 221,884 bytes and a different hash (`48562c554fe8ec0d29a4aa10222eceec53d36954d10f9c7cea0f124a5992f41e`); CI rejects a host path, not that difference.

I get the storage back from the wallet, not from the factory. I pause it, withdraw every token, and call `close` with a beneficiary that already exists. Account deletion returns the code stake, the state, and the working balance. The factory's own code stake stays until I delete the factory account.

This build's factory wasm is 245,448 bytes after `wasm-opt`, sha256 `8017146f1715e167e2e3b3e2da1d9f6f8e3cb66d03912f415f406dbf7b5be1b6`. The dry-run peak for that file is 6.41058 NEAR: 2.50174 to fund the factory, 0.08000 of deploy burn, the 3.52884 create deposit, and 0.30000 of prepaid gas. A rebuild can move the factory size by a few bytes; the script prints the figure for the wasm it finds.

The mainnet script is `scripts/mainnet/mainnet-e2e.mjs`. `--dry-run` prints the budget and does not read a key. A real run reads `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` from the file in `APS_MAINNET_ENV` and never prints the key. It deploys the factory, creates one wallet as the owner (the fee comes back), moves 0.01 USDC and a millionth of a wNEAR, checks holds and a revoked key, then closes the wallet. It does not create a second wallet. The projected peak, factory fund plus one create plus gas the signer has to be holding, is under 6.5 NEAR. The dry-run prints the exact figure from the wasm it would deploy.

```text
cd contracts
rustup target add wasm32-unknown-unknown
REG=$(echo "$CARGO_HOME"/registry/src/index.crates.io-*)
RUSTFLAGS="--cfg near --remap-path-prefix=${CARGO_HOME}=/cargo --remap-path-prefix=${RUSTUP_HOME}=/rustup --remap-path-prefix=/usr/local/cargo=/cargo --remap-path-prefix=/usr/local/rustup=/rustup --remap-path-prefix=$PWD=/src --remap-path-prefix=${REG}=/cargo/registry/src" \
  cargo build -p policy-wallet -p wallet-factory --target wasm32-unknown-unknown --release
wasm-opt -Oz --strip-debug --strip-producers --enable-bulk-memory \
  --enable-nontrapping-float-to-int --enable-sign-ext \
  target/wasm32-unknown-unknown/release/wallet_factory.wasm \
  -o target/near/wallet_factory/wallet_factory.wasm
cd ..
node scripts/mainnet/mainnet-e2e.mjs --dry-run
APS_MAINNET_ENV=/path/outside/the/repo.env \
  node scripts/mainnet/mainnet-e2e.mjs --out-dir DIR --keys-dir /path/outside/keys
```

`wasm-opt` is binaryen 108. The release profile is `opt-level = "z"`, `lto = true`, `strip = true`, `panic = "abort"`, `debug = false`. The page at `/app` states the same 1 NEAR fee and 2.52884 NEAR of refundable storage. It also requests a NEAR Intents 1Click quote that settles as USDC on the wallet. The deposit is signed in the user's own wallet; the page does not connect one. The contract is beta and unaudited. The deposit ceiling on the wallet is 100 USDC and 10 wNEAR, and a raw `ft_transfer` can bypass it.

## Hosted demo

`demo/live` is the judge page at https://triagepay-demo.vercel.app/. Press Launch the attack and the server signs `pay` on NEAR testnet. The page shows the attack path, live spend, a receipt, and a replay URL. The browser never sees a key. I give the model two seconds on that launch, then use the scripted agent, so the refusal is not waiting on a slow model. Drain Me still uses the full model budget.

What is on the page, and what it needs:

| Feature | Status | Env |
|---|---|---|
| Launch the attack, scenarios, payee risk, Revoke and Restore | Live on testnet when the two signing keys are set | `TRIAGEPAY_AGENT_SECRET_KEY`, `TRIAGEPAY_OWNER_SECRET_KEY` |
| Owner review (hold, Approve, Reject) | Live on the testnet wallet. The threshold is shown in wNEAR. Approve on the public page releases only a payment to the allowlisted payee within the per-call cap; Reject works for any held payment | the owner key; `TRIAGEPAY_OWNER_ACTION_TOKEN` to approve anything else |
| Drain Me | A visitor's text runs through the agent and the wallet. I do not sign a payment the wallet would settle | the agent key; per-address cap `TRIAGEPAY_DRAIN_IP_LIMIT` (default 8) |
| Scripted agent | Default. The page says `scripted` | none |
| NEAR AI agent | Used when the key is set. A timeout or an error tries the next model in `NEARAI_MODEL` inside a 45s budget, then the scripted agent. The page and `/api/health` say whether the key was missing, the call timed out, or the model returned an error. Health also shows the last call's model and latency, not the key | `NEARAI_API_KEY`, optional `NEARAI_MODEL` (comma-separated; default `z-ai/glm-5.3-flash`, then `Qwen/Qwen3.8-27B`) |
| Receipts and the PNG share card | No secret. Share text includes the hackathon line | none |
| Attack replay | `/replay/<tx>` plays about ten seconds. A stored narrative is used when this server signed the transaction. Otherwise I rebuild the payee, the amount, the error code, and the demo sentence from the transaction | shared store optional |
| Honeypot payee | The injected account is the trap. The list is the agent's refused `pay` calls to that account, read from the chain index and the RPC. The deployed wallet has no attacker registry of its own | optional `TRIAGEPAY_HONEYPOT_ACCOUNT` (defaults to the injected account) |
| Telegram approval | A hold can message the owner with Approve and Reject. Off until the three variables are set | `TELEGRAM_BOT_TOKEN`, `TELEGRAM_OWNER_CHAT_ID`, `TELEGRAM_WEBHOOK_SECRET` |
| Kill switch | An expiring single-use link and a QR code. Off until the secret is set. Revoke on the page still works | `KILL_SWITCH_SECRET` |
| Natural-language policy | Preview always. Apply signs `set_agent` / `set_review_policy` only with the flag, the owner token and the preview's `confirm` digest, and only inside the `config.json` bounds. It is off on the public demo | `TRIAGEPAY_POLICY_APPLY=1` and `TRIAGEPAY_OWNER_ACTION_TOKEN`; `NEARAI_API_KEY` optional, otherwise the parser |
| Agent passport | `/agent/<account>` and `GET /api/passport?account=`. Limits from the wallet. Rows from the indexer. Error codes only when this server's journal has them | none |

The demo accounts on testnet: wallet `tpd-e947e7-wallet.testnet` (with the owner review queue), agent `tpd-e947e7-agent.testnet`, owner `tpd-e947e7-owner.testnet`. The agent's only key is the pay-only function-call key. `/api/health` shows which wallet the page is using.

An allowlisted payee shows warn when the account is under seven days old. The demo payee was created for the page, so warn is expected; warn does not move money. If a visitor leaves the agent revoked, the next Launch restores it before the attack, and a daily cron calls the same restore.

The same wallet also refused [an over-limit payment (`ERR_PER_CALL_LIMIT`)](https://testnet.nearblocks.io/txns/BMoHjQXgQasLxn4wZspzTQU9g1rBjbqThhZQevknCZ7s) and, after [a revoke](https://testnet.nearblocks.io/txns/5qZ5yQAPTm5dFg7i455HvsrgBNk8bTbbCou6zq4461as), [a revoked agent (`ERR_AGENT_INACTIVE`)](https://testnet.nearblocks.io/txns/3iHytU9nkYuqPh74its9TPW3MwQoUJeaPQg1nbGGyQTZ).

Running or deploying it yourself (environment variables, rate limits, the shared store, setup, and how the hosted keys were moved): [docs/demo/hosted.md](docs/demo/hosted.md).

## Chain Signatures

[`v1.signer-prod.testnet` signed a Base Sepolia USDC transfer](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM) for the owner account, and the signature recovers to the derived address `0x508e984fda3bc37d8c33bfb5098441fd2169fcc8`. It was not broadcast, and the wallet contract does not call `sign` yet ([docs/contracts/chain-signatures.md](docs/contracts/chain-signatures.md)).

## The problem

An agent that holds a normal NEAR key can empty its account the first time a prompt injection tells it to. A function-call key cannot move tokens either: it cannot attach the 1 yoctoNEAR that NEP-141 `ft_transfer` requires. So the agent needs a contract between its key and the money.

## Try it

### A. Offline, no keys, about two minutes

Needs Rust (stable) and Python 3.12.

| Do | See |
|---|---|
| `cargo test --workspace` | `237 passed`, `3 ignored` (the ignored three read mainnet RPC) |
| Terminal 1: `cargo run -p aps-api --bin aps-api` | `aps-api listening on http://127.0.0.1:8080 (payment provider: mock)` |
| Terminal 2: `python scripts/x402-demo.py` | Step 1: `HTTP 402` and the decoded payment requirement. Step 2: `HTTP 200`, `verdict: needs_evidence`, the content hash the payment settled against. Step 3: the same proof again returns the identical response. Step 4: the proof reused for another finding is `proof_already_consumed` |
| `./scripts/demo.sh` (or `demo.ps1`) | The prepaid flow without x402: fund, reserve, scan, settle, reject, refund; final balances conserve |

The mock provider accepts any transaction hash on a development-only network (`aps:mock`). It shows the HTTP exchange and the ledger, not payment verification.

To add the NEAR AI judge without a key, start the API with `APS_SCAN_SKU=triage.v2` and `APS_TRIAGE_CASSETTES=triage-model/cassettes`: the walkthrough then also prints the judge's opinion and rationale, replayed from recorded replies. With `APS_LIVE_MODEL=1` and `NEARAI_API_KEY` set instead, the call goes to NEAR AI Cloud (about USD 0.0002 per call). Details: [docs/engine/overview.md](docs/engine/overview.md).

### B. Read the testnet run, no setup

[docs/demo/transcript-2026-11-11.md](docs/demo/transcript-2026-11-11.md) is the unedited output of the agent demo on NEAR testnet. Every step links to its transaction. In order:

1. The wallet account has 0 access keys. The agent's key is `FunctionCall{receiver: wallet, methods: [pay]}`.
2. The agent pays for four triage calls. Each is a 402, a wallet `pay`, and a proof the server accepts only after reading the token contract's transfer event. Verdicts: `needs_evidence`, `supported`, `supported` with embedded instructions flagged, `rejected`.
3. A finding tells the agent to send a token to another account. The agent obeys; the wallet answers `ERR_PAYEE_NOT_ALLOWED` and no balance moves.
4. One atomic unit over the per-call limit: `ERR_PER_CALL_LIMIT`.
5. A fifth call the same day: `ERR_DAILY_LIMIT`.
6. The agent calls the owner method `set_agent` to raise its own limit. The node rejects the transaction: a function-call key cannot attach the required deposit, and the key does not list the method.
7. The owner calls `revoke_agent`. The agent's key still exists and its next `pay` fails with `ERR_AGENT_INACTIVE`.

### C. Re-run it on testnet, about ten minutes

Needs Node 22.18 or later, `cargo-near` 0.21.1 with the `wasm32-unknown-unknown` target, and the key file of a testnet account holding about 6 NEAR (`{"account_id": "...", "secret_key": "ed25519:..."}`, kept outside the repository). The script creates throwaway accounts and never touches mainnet.

```sh
npm ci --prefix clients/typescript
(cd contracts/policy-wallet && cargo near build non-reproducible-wasm --locked --no-abi)
(cd contracts/test-token && cargo near build non-reproducible-wasm --locked --no-abi)
node agents/demo/demo.mjs setup --out-dir ../demo-run --funder-key ../funder.json
# setup prints the line that starts the API with the new gateway and token ids
node agents/demo/demo.mjs run    --out-dir ../demo-run --api http://127.0.0.1:8080
node agents/demo/demo.mjs direct --out-dir ../demo-run --api http://127.0.0.1:8080
node agents/demo/demo.mjs revoke --out-dir ../demo-run --api http://127.0.0.1:8080
```

`../demo-run/transcript.md` then reads like the file in B, with new transaction hashes. Details: [agents/demo/README.md](agents/demo/README.md).

## What NEAR does here

| NEAR feature | Use | Proof |
|---|---|---|
| Function-call access keys | The key the agent uses to pay: one contract, one method, no deposit. On the hosted agent account it is the only key | Transcript steps `keys_view`, `self_escalation`; [docs/contracts/policy-wallet.md](docs/contracts/policy-wallet.md) testnet steps 11-24; [docs/demo/hosted.md](docs/demo/hosted.md) for the hosted keys |
| A contract as the spending policy | Per-call limit, per-UTC-day limit, payee allowlist, pause, revoke, and an owner queue for a new payee or an amount above a threshold; owner methods need 1 yoctoNEAR, which no function-call key can attach | `contracts/policy-wallet`: 94 unit tests and 1 ignored design-issue test, 23 sandbox tests, 3 property tests in `crates/policy-core` plus one proptest over the wallet; transcript refusals |
| Chain Signatures | One testnet `sign` request on `v1.signer-prod.testnet` for an EIP-1559 USDC transfer on Base Sepolia; the signature recovers to the address derived for (owner account, `base-sepolia-1`). Not broadcast, and the wallet does not call `sign` yet | [docs/contracts/chain-signatures.md](docs/contracts/chain-signatures.md); [sign transaction](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM); SDK tests reuse the recorded values |
| NEP-141 and NEP-297 events | Payment is an `ft_transfer`; the server accepts it only when the token contract's own `ft_transfer` event, in a finalized receipt, pays the gateway with the server-issued memo | `crates/near-adapter/src/verify.rs`; 21 receipt-fixture tests, one a real mainnet capture; transcript |
| x402 v2 over NEAR | A client-submitted payment proof (`near-tx-proof`) that works from a policy wallet. The upstream NEAR binding needs a full-access key; this one does not. A proposed profile, not upstream | [docs/protocols/x402-near.md](docs/protocols/x402-near.md); 16 x402 flow tests; transcript |
| NEP-413 signed messages | Login-message verification for future caller authentication | `crates/auth`, 25 tests. A library: the API does not use it yet |
| NEAR AI Cloud | The optional Stage B judge (`APS_SCAN_SKU=triage.v2`) calls a model the catalogue lists as TEE-verifiable; it is advisory and can never raise a verdict. The hosted demo separately checks that an attestation report echoes a fresh nonce. The Intel quote is not verified | Spike on 2026-11-12: 50 live calls, 5 models, USD 0.096 ([docs/engine/model-selection.md](docs/engine/model-selection.md)); recorded replies in `triage-model/cassettes`, replayed by 6 tests. Paid route: no attestation (ADR-0011). Demo check: [docs/engine/attestation.md](docs/engine/attestation.md) |

## What the agent's key can and cannot do

- It can call `pay(token, receiver, amount, memo)` on one wallet, with no deposit. The wallet checks, in order: not paused, agent active, token allowlisted, payee allowlisted, amount within the per-call limit, day total within the daily limit. Then the wallet itself sends the `ft_transfer`. With owner review on, a new payee or an amount above the threshold is queued instead of transferred ([docs/contracts/review-policy.md](docs/contracts/review-policy.md)).
- On the hosted agent account (`tpd-e947e7-agent.testnet`) this is the only key. [docs/demo/hosted.md](docs/demo/hosted.md) has the deletion transaction.
- It cannot call any owner method, transfer NEAR, add or delete keys, or deploy code. Those need a deposit or a full-access key.
- Stolen, the pay-only key can spend at most the daily limit per UTC day, to allowlisted payees only, until the owner revokes the agent or pauses the wallet. Across a UTC midnight that is up to two daily limits within minutes.
- It carries a gas allowance. On mainnet the allowance falls by the gas each transaction burns, but a 40 TGas `pay` needs about 0.041 NEAR of allowance and balance up front (measured 2026-11-13). When the allowance runs out the key stops working until the account's full-access key replaces it. An account that holds only function-call keys can lock itself out this way. The hosted agent is such an account.
- Revocation happens at the wallet (`revoke_agent`). The wallet owner cannot delete a key on the agent's account; the demo shows key deletion as a separate step by the agent account.
- `tp-wallet` holds one token. Wallets from the factory allow up to 8, each with its own per-call and daily limit in that token's atomic units. In the current source, changing the token list never resets a token's daily total. That fix is not deployed (see [Status](#status)).
- The owner key is not limited: it can withdraw everything. The wallet account itself has no keys and no upgrade method.

## How the server knows it was paid

1. `POST /v2/scan` without payment creates a deposit intent and answers 402. The requirement names the asset, the payee, the exact amount and a memo that ends in a 128-bit random nonce.
2. The agent pays with that memo, directly or through the wallet, and retries with the transaction hash.
3. The server reads the transaction with `wait_until: FINAL` and looks for a successful receipt executed by the asset contract, carrying an `ft_transfer` call with 1 yoctoNEAR and exactly one `nep141` event to the payee with the memo. Events logged by any other contract are ignored. `ft_transfer_call` is refused because it can be refunded.
4. The exact amount funds the intent once. A wrong amount, a second payment or a late one is recorded as an orphan for an operator-approved refund and never becomes credit.
5. The ledger reserves the price, runs the scan, and settles against the report's content hash. The same proof and request replay the stored response; anything else is refused.

Money is integer-only (u128 atomic units). Every balance satisfies `deposited == available + reserved + settled`, checked after every operation by a randomized test.

## Status

| | |
|---|---|
| Run on testnet | Policy wallet (28-step run on 2026-11-06, and the agent demo on 2026-11-11); the owner-review wallet on 2026-11-12 (held, approved, rejected) and again from the production page on 2026-11-13; one Chain Signatures request on 2026-11-12, signed and not broadcast; x402 round trips paid through the wallet and directly; receipt verification |
| Run on mainnet | The review-queue policy wallet, live since 2026-11-13: 166 graded on-chain assertions, none failing, with 183 transactions ([re-graded](docs/demo/mainnet-campaign/2026-11-13/REGRADE.md); the first count of 170 included 4 model rounds that never tried to pay), including 51 NEAR AI completions that drove the agent's `pay` calls ([Live on mainnet](#live-on-mainnet)). Before that, the first mainnet wallet on 2026-11-12: one paid call, one refused payee, revoke, one revoked-agent refusal, then closed. I re-check the live views every six hours. Earlier, one native-USDC deposit verified on 2026-10-21 by the argument-based verifier ([docs/integrations/near.md](docs/integrations/near.md)). x402 on mainnet: one paid scan on 2026-11-14 in native USDC through the factory-created wallet, verified from the USDC contract's event, plus replay and reuse checks ([CwLaJmaX](https://nearblocks.io/txns/CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG), [evidence](docs/demo/mainnet-x402/2026-11-14/README.md)); the server ran locally |
| Run against NEAR AI Cloud | Stage B: a 50-call spike on 2026-11-12 and the paid route in `triage.v2` mode. Every verdict in the 2026-11-11 testnet transcript comes from Stage A alone; the run predates the working key |
| Built, tested offline only | The x402 facilitator endpoints, the SQLite journal (`crates/store`), NEP-413 and API keys (`crates/auth`), the static UI (`ui/`), the Dockerfile |
| Not wired | The API keeps its ledger in memory, serves one development caller with no authentication, binds loopback only, and does not serve the UI |
| Run in CI only | The 23 sandbox tests in `contracts/tests` (near-workspaces, a local NEAR node) run on GitHub Actions, Linux. The sandbox has no Windows build |
| Falsara on mainnet | Live: the NEAR gateway (`services/falsara-near`) and the Falsara core on Vercel. Two paid checks on 2026-11-14 through the factory-created wallet, both `REFUSE` ([AZ9kPAz8](https://nearblocks.io/txns/AZ9kPAz84NGpyqhNjibBHKNtFGthMwH58hHiV4Pgr74U) and [49Jhqhsz](https://nearblocks.io/txns/49JhqhszsNUdiny8EStKEWD1bWsfc3eNYUFSCgw4Qz9R)); funding and caps set by the owner the same day ([record](docs/demo/falsara-near/README.md)) |
| Hosted judge demo | `demo/live`: Vercel static page plus serverless functions on testnet. Signing keys are environment variables. The cross-instance rate-limit store is not connected yet (`sharedLimits: false`) |
| Red-team review | An independent review on 2026-11-13 found no Critical and no way to move funds past the wallet's caps. Its 3 High, 7 Medium and 22 Low or Info findings around the contract are fixed or documented, each with a regression test: [record](docs/security/redteam-2026-11-13.md) |
| Trail of Bits skill run | On 2026-11-14, nine Trail of Bits audit skills ran over the repository. Of 63 candidates, 21 were real defects and one was reachable by an outsider: the Falsara gateway accepted a refunded `ft_transfer_call` as payment, so a paid check could be had for gas. That is fixed and live since the same day. The others needed the owner's own action or were documentation. They are fixed in the source and the docs, with regression tests. The wallet fixes are not on mainnet, because the deployed contracts have no upgrade path: [record](docs/security/tob-skills-2026-11-14.md) |
| Cut | Attested verdicts on the paid route (ADR-0011); the demo checks a nonce echo and does not verify the Intel quote. Also cut: NEAR Intents deposits and a measured accuracy figure. No accuracy is claimed anywhere |

Known limits of the paid route: no rate limiting; the server trusts its RPC endpoint; on `/v2/scan` the envelope is validated after payment (an invalid one keeps the payment claimed and can be retried with a corrected body); The hosted demo calls a NEAR AI model only when `NEARAI_API_KEY` is set, and otherwise uses a scripted agent and says so.

## Hackathon window and what is reused

The first commit is dated 2026-10-20. The rules count code from the announcement on 2026-10-19 (Q-001), so no code here predates the window. `git log --reverse` shows the whole history.

**Falsara (Q-002, Q-015).** Falsara is an existing product of mine on another chain: its core, built from August 2026, took x402 payment on Base only. Only its NEAR integration is new and is the part to judge: the gateway in `services/falsara-near` (this repository, every line written on 2026-11-14), the serverless packaging of the core (Falsara's private repository, `near` branch, 2026-11-14), and the TriagePay agent that pays it. The gates Falsara runs, viability-gate and realism-gate, are my own public libraries and count as baseline (Q-005).

Libraries: axum, tokio, serde, ureq, base64, getrandom, ed25519-dalek, sha2, bs58, rusqlite (bundled SQLite), proptest; near-sdk 5.29.1, near-contract-standards and near-workspaces for the contracts; near-api-js 7.3.1 for the clients and testnet scripts. The Python packages use the standard library only. The x402 work follows the public x402 v2 specification; the `near-tx-proof` method is my own proposal.

I am one developer and I wrote this with AI coding agents (Claude Code). The commits carry the co-author trailer.

## Tests

Counts are from runs on 2026-11-14 on `main`, except the contracts row (2026-11-13; the contracts did not change since) and the sandbox, which needs Linux and runs in CI.

| Suite | Command | Result |
|---|---|---|
| Rust workspace | `cargo test --workspace` | 237 passed, 3 ignored |
| Contracts | `cd contracts && cargo test` | policy-wallet 94 passed, 1 ignored (the open review-queue availability issue, [PR #7](https://github.com/KhomDev/TriagePay/pull/7)); test-token 3 passed; sandbox 23 tests, run in the contracts workflow on Linux |
| Scanner | `cd scanner && python -m pytest -q` | 322 passed |
| Knowledge compiler | `cd knowledge-compiler && python -m pytest -q` | 72 passed, 1 skipped |
| Model judge (offline, with cassette replay) | `cd triage-model && python -m pytest -q` | 74 passed |
| OpenAPI and Python client | `python -m pytest tests -q` | 14 passed |
| UI | `python -m pytest ui/tests -q` | 17 passed |
| TypeScript client | `cd clients/typescript && npm ci && npm test` | 7 passed |
| Hosted demo | `cd demo/live && npm test` | 121 passed (`npm run build` rebuilds `app.min.js`; a test fails when it is stale) |
| Policy-wallet SDK | `cd packages/triagepay-sdk && npm test` | 13 passed |
| Falsara NEAR gateway | `cd services/falsara-near && npm test` | 12 passed (the verifier runs against real mainnet receipts) |
| Mainnet watch | `node scripts/mainnet/live-check.mjs` | read-only; the local run on 2026-11-14 passed |
| Mainnet scripts | `node --test scripts/mainnet/*.test.mjs` | 9 passed |

The claim-by-claim record is [docs/claims-audit.md](docs/claims-audit.md).

## Layout

```
contracts/policy-wallet  the spend-policy wallet (near-sdk)
contracts/test-token     NEP-141 test token used on testnet and in sandbox tests
crates/policy-core       the wallet's limit arithmetic, dependency-free
crates/core              integer-money prepaid ledger, dependency-free
crates/payment           provider trait, reconciliation, mock provider
crates/near-adapter      NEAR provider: intents, RPC, receipt verification
crates/api               HTTP API: prepaid routes, /v2/scan (x402), facilitator
crates/store, crates/auth  SQLite journal; NEP-413 and API keys (not wired)
scanner/                 triage engine: validation, Stage A gates, reports
triage-model/            Stage B model judge on NEAR AI Cloud, recorded cassettes
knowledge-compiler/      compiles rule sources into hash-addressed rulesets
clients/                 x402 reference clients (TypeScript, Python)
agents/demo/             the testnet agent demo (full x402 loop, throwaway accounts)
demo/live                hosted judge demo (Vercel): attack, replay, honeypot list, policy preview, passport
services/falsara-near    Falsara on NEAR: the x402 gateway that takes USDC on mainnet (Vercel)
packages/triagepay-sdk   one-line policy-wallet payer, x402 helper, NEAR AI tool
ui/                      static pages (not served by the API yet)
docs/                    ADRs, protocol profile, contract docs, transcript
```

## HTTP surface

| Route | Effect |
|---|---|
| `POST /v2/scan` | The paid scan behind HTTP 402 (x402 v2, `near-tx-proof`) |
| `GET /x402/supported`, `POST /x402/verify`, `POST /x402/settle` | Facilitator endpoints for third-party servers |
| `POST /payment/intents`, `GET /payment/intents/{id}`, `POST .../observations`, `POST .../refunds` | Prepaid funding from provider-observed deposits; refunds are proposals |
| `POST /reservations`, `GET /jobs/{id}`, `POST /jobs/{id}/start`, `/scan`, `/refunds` | Prepaid job lifecycle |
| `GET /accounts/{id}`, `GET /health` | Balances; liveness |
| `POST /accounts/{id}/deposits`, `POST /jobs/{id}/settlements` | Development only: 403 unless the mock provider is active |

Full contract: [docs/openapi.json](docs/openapi.json). One error shape
everywhere: `{"error":{"code","message"}}`.

| Variable | Default | Meaning |
|---|---|---|
| `APS_PORT` | 8080 | Listen port (loopback only) |
| `APS_PROVIDER` | mock | `mock` or `near` |
| `NEAR_NETWORK` | none | `mainnet` or `testnet` (near only) |
| `NEAR_GATEWAY_ACCOUNT` | none | Account that receives payments (near only) |
| `NEAR_ASSET_CONTRACT` | none | NEP-141 settlement asset; it must emit NEP-297 events (near only) |
| `NEAR_RPC_URL` | FASTNEAR free endpoint for the network | JSON-RPC endpoint |
| `APS_X402_PRICE_ATOMIC`, `APS_X402_MAX_TIMEOUT_SECS` | 1000000, 900 | Price of one call; requirement validity |
| `APS_SCANNER_PYTHON`, `APS_SCANNER_DIR`, `APS_SCANNER_TIMEOUT_SECS` | discovered, discovered, 30 | Scanner subprocess (use 120 with a live model) |
| `APS_SCAN_SKU` | scan.v1 | `triage.v2` adds the Stage B model judge |
| `APS_LIVE_MODEL`, `NEARAI_API_KEY` | unset | Both set: live Stage B calls to NEAR AI Cloud |
| `APS_TRIAGE_CASSETTES` | unset | Replay Stage B from a cassette directory, no key |
| `APS_TRIAGE_MODEL_ID` | z-ai/glm-5.3-flash | Stage B model |

## Sister project

The same team also built [Vibe Wallet](https://vibe-wallet-flame.vercel.app) ([repository](https://github.com/KhomDev/vibe-wallet)) at NEAR HACKATHON. TriagePay and Vibe Wallet are the two projects from this team; both are submitted by Yura (Yuriy Khomenkov). Falsara is not a separate submission: its NEAR integration is part of TriagePay (see above).

## More

[Architecture](docs/architecture.md) |
[threat model](docs/threat-model.md) |
[red-team record](docs/security/redteam-2026-11-13.md) |
[Trail of Bits skill run](docs/security/tob-skills-2026-11-14.md) |
[wallet](docs/contracts/policy-wallet.md) |
[x402 profile](docs/protocols/x402-near.md) |
[engine](docs/engine/overview.md) |
[hosted demo](docs/demo/hosted.md) |
[decisions](docs/decisions/)

## Team

Hackathon submitter (this project and [Vibe Wallet](https://github.com/KhomDev/vibe-wallet)):

- **Yura (Yuriy Khomenkov)** — team lead · email [yuriykhomekov@gmail.com](mailto:yuriykhomekov@gmail.com) · GitHub [@KhomDev](https://github.com/KhomDev) · X [@YKhomenkov](https://x.com/YKhomenkov)

## License

MIT. See [LICENSE](LICENSE).
