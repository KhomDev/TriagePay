# TriagePay on NEAR mainnet (2026-11-12)

A small, real run of the policy wallet on mainnet. I paid with wNEAR in
tiny amounts. Nothing here is a test fixture.

**Superseded on 2026-11-13.** I closed this wallet during the mainnet campaign. I captured its final state first. Then I re-created
`tp-wallet.hlpok1lad.tg` with the review-queue build. The transactions below
still resolve on the explorer, but the code they ran is no longer deployed. The current state and 170 further mainnet tests are in
[`docs/demo/mainnet-campaign/2026-11-13/`](../../mainnet-campaign/2026-11-13/README.md).

- Script: `scripts/mainnet/mainnet-proof.mjs` (`setup`, `paid`, `refuse`, `revoke`, `status`, `close`)
- Evidence: `mainnet-EVIDENCE.json` (every step, tx hash, outcome, events, gas); readable log: `transcript.md`
- Keys: none in this folder. The owner key was read from a local file and never printed; the generated keys live outside the repository.
- Code: the policy wallet built from this repository at `82d64e3` (contracts unchanged since the testnet runs), 174,803 bytes, SHA-256 `26c2ed4984af3ef855a6968fd20c5a25a50bdc98cf469ff629f0b43ace75c68f` (Windows host build; Linux builds hash differently, see `docs/contracts/build.md`).

## Accounts

| Role | Account |
|---|---|
| Owner | [`hlpok1lad.tg`](https://nearblocks.io/address/hlpok1lad.tg) |
| Policy wallet (contract, 0 access keys) | [`tp-wallet.hlpok1lad.tg`](https://nearblocks.io/address/tp-wallet.hlpok1lad.tg) |
| Agent (one key: function call, `pay` on the wallet only) | [`tp-agent2.hlpok1lad.tg`](https://nearblocks.io/address/tp-agent2.hlpok1lad.tg) |
| Retired first agent (allowance too small, revoked) | [`tp-agent.hlpok1lad.tg`](https://nearblocks.io/address/tp-agent.hlpok1lad.tg) |
| Allowlisted payee | [`gateway.hlpok1lad.tg`](https://nearblocks.io/address/gateway.hlpok1lad.tg) |
| Token | [`wrap.near`](https://nearblocks.io/address/wrap.near) (wNEAR, 24 decimals) |
| Injected payee (never created) | `tp-mallory.hlpok1lad.tg` |

Policy: 1,000,000 yocto-wNEAR per call, 4,000,000 per UTC day, payees `[gateway.hlpok1lad.tg]`, token `[wrap.near]`.

## The proof

| Step | Result | Tx |
|---|---|---|
| Agent pays the allowlisted payee 1,000,000 yocto-wNEAR | success; `pay_requested` + `pay_settled`; wallet -1,000,000, payee +1,000,000 | [2RH77VLs…](https://nearblocks.io/txns/2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ) |
| Agent follows an injected instruction: `pay` to `tp-mallory.hlpok1lad.tg` | refused, `ERR_PAYEE_NOT_ALLOWED`; wallet balance change 0 | [HskzPv94…](https://nearblocks.io/txns/HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F) |
| Owner revokes the agent | success | [CFutaZM9…](https://nearblocks.io/txns/CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh) |
| Revoked agent's key (still on chain) tries `pay` to the allowlisted payee | refused, `ERR_AGENT_INACTIVE`; wallet balance change 0 | [C1BnCCrE…](https://nearblocks.io/txns/C1BnCCrEMYfxhirf283GfGr2sfeBMaZ88fimxQwoKt2d) |

## Setup transactions

| Step | Tx |
|---|---|
| Create the wallet account (1.765 NEAR, temporary deploy key) | [GXQ9hSsn…](https://nearblocks.io/txns/GXQ9hSsnWX6DrqxTfgoLCQW1vQzkMMtf4WPuaB34wLX3) |
| First deploy: failed, `LackBalanceForState` (short 0.0276 NEAR), batch reverted | [3XJHm2D3…](https://nearblocks.io/txns/3XJHm2D329zGHFie2GwouKTaJDFKDZ3tknZHLXM7PE5b) |
| Top up the wallet account (0.045 NEAR) | [G7Rkcc…](https://nearblocks.io/txns/G7RkccZNjEso4uxUAfi2x6iGpW5qzYjiwtdVx7EkkazV) |
| Deploy, `new(owner, [wrap.near])`, delete the wallet's only key (one batch) | [AXan6pvj…](https://nearblocks.io/txns/AXan6pvjF3sFUtRYbKHAXqabKYrFsy3CEmvojF18Jqfn) |
| Create the first agent `tp-agent` (0.05 NEAR, allowance 0.04) | [C9ua1ezi…](https://nearblocks.io/txns/C9ua1ezirZiXDzxw4Dar9AeBucp1ahTrr2j4iQGmvhDu) |
| Wrap 0.002 NEAR, register the wallet and the payee on `wrap.near` | [DnTfeQQE…](https://nearblocks.io/txns/DnTfeQQEBH5yTwXDHK5jNAQTwGK4XRMB8Z7qKpaULFqN) |
| Fund the wallet with 0.001 wNEAR (`ft_transfer_call`) | [Ef1iGZ1F…](https://nearblocks.io/txns/Ef1iGZ1FBVNxn21Tf4JYwLknYYKsf9hN3xXhz3LFTun2) |
| Policy for `tp-agent` | [FrJxyT7K…](https://nearblocks.io/txns/FrJxyT7KaDsgB2bAeKEQJpXBExUzYCrGCaVn1SNDwf46) |
| Create `tp-agent2` (0.07 NEAR, allowance 0.15, one pay-only key) | [APN65xR3…](https://nearblocks.io/txns/APN65xR3a2J9UgQ9fRSBnJZ4P8Nx4QdLMeZLgpWMj6Vh) |
| Policy for `tp-agent2` | [76EibxbU…](https://nearblocks.io/txns/76EibxbULaWnfdtVD2rq5q15QSM7M3WWe3NzjfPQA65s) |
| Revoke the retired `tp-agent` | [6LpSznrm…](https://nearblocks.io/txns/6LpSznrmVSBEzyCS51xr8C5a2k2LN6VUKvSvmX8DGSf8) |

## What went wrong, and what it cost

- The first deploy ran short: code plus initial state needs more than the code's 1 NEAR per 100 KB. A 0.045 NEAR top-up fixed it.
- The first agent's key could not pay at all. A transaction needs the full prepaid gas at the pessimistic price (about 0.001 NEAR per TGas on mainnet) available up front. 50 TGas cost 0.0509 NEAR against a 0.04 allowance and a 0.05 balance. (Correction, 2026-11-13: I wrote here that refunds do not go back to the allowance. Over the campaign, `tp-agent2`'s allowance fell by exactly its gas burnt, 0.036838 NEAR. Only the up-front check is pessimistic.) With no full-access key the key cannot be changed, so I retired `tp-agent` and created `tp-agent2`.
- The owner went from 2.627341 to 0.669622 NEAR (down 1.957719): 1.810 NEAR sits in the wallet account, 0.12 in the two agent accounts, 0.002 wrapped, 0.0025 of wNEAR storage, about 0.023 burnt as gas.
- No x402 receipt check on mainnet: the API verifies a payment from the token contract's own NEP-141 `ft_transfer` event, and `wrap.near` emits none (its `ft_transfer_call` logged no NEP-297 event). The x402 round trip is proven on testnet; on mainnet it needs an event-emitting token such as native USDC.

## Closing the wallet later

The wallet stays live for judges. To recover its deposit (about 1.807 NEAR) the owner runs:

```
APS_MAINNET_ENV=<path to the owner's env file> node scripts/mainnet/mainnet-proof.mjs close --out-dir docs/demo/mainnet-e2e/2026-11-12 --keys-dir <keys dir outside the repo>
```

That is `pause()`, then `withdraw(wrap.near, <whole balance>, hlpok1lad.tg)`, then `close(beneficiary = hlpok1lad.tg)`, each from the owner with 1 yoctoNEAR. `close` deletes the account only when the wNEAR balance is zero. The agent accounts have no full-access key, so their NEAR (about 0.12) cannot be recovered.