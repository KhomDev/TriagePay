# Agent demo transcript, NEAR testnet, 2026-11-11

This file is the output of `agents/demo/demo.mjs` (commands `setup`, `run`,
`direct`, `revoke`), unedited below the first rule, followed by three live
checks of the 2026-11-11 money-path fixes run against the same server.
Every line with a link is a testnet transaction; a line without one was
refused by the node before execution, so there is no transaction to link.

| Item | Value |
|---|---|
| Network | NEAR testnet, RPC `https://test.rpc.fastnear.com` |
| Value | zero: testnet NEAR and a test token deployed for the run |
| Wallet contract | `tpd-081ea8-wallet.testnet`, wasm 174,618 bytes, SHA-256 `baad3c689a3638b50602781aa62bc9e221643d7a437f19eecbd20d7968773130` (the build in `docs/contracts/build.md`) |
| Token | `tpd-081ea8-token.testnet`: `contracts/test-token` (near-contract-standards NEP-141, 6 decimals), wasm SHA-256 `a8af49a519b52556fb9e8623231528e4922f9b5bb38951f509442c5ab83c07a3` |
| Owner / agent / gateway | `tpd-081ea8-owner.testnet` / `tpd-081ea8-agent.testnet` / `tpd-081ea8-gateway.testnet` |
| Policy | per call 1.000000, per UTC day 4.000000, payees: the gateway only |
| Server | `aps-api` at commit `d018cc4` plus the demo script, `APS_PROVIDER=near`, `NEAR_NETWORK=testnet` |
| Agent | a fixed loop with no language model; one deliberately naive behaviour (it follows a payment instruction found in a finding) |

Why a test token and not `wrap.testnet`: the receipt verifier accepts only
the token contract's own NEP-297 `nep141` `ft_transfer` event, and
`wrap.testnet` logs plain text instead of events.

---

# TriagePay agent demo: transcript

NEAR testnet, zero value, throwaway accounts. RPC https://test.rpc.fastnear.com. Started 2026-11-11T10:58:20.759Z.

## Setup

- **create_owner**: create `tpd-081ea8-owner.testnet` with 0.5 NEAR ([tx](https://testnet.nearblocks.io/txns/3C3v2dwuscA4cY5zq8x39Yhe1jUfvEQuWAcRsReJXpEj))
- **create_agent**: create `tpd-081ea8-agent.testnet` with 1 NEAR ([tx](https://testnet.nearblocks.io/txns/GwwPUcwPSbyNVwJA3knsE7G1WTfpa33d3bmAtri3fMUi))
- **create_gateway**: create `tpd-081ea8-gateway.testnet` with 0.05 NEAR ([tx](https://testnet.nearblocks.io/txns/GAFvEKCueQSf13QPVPaYpkfdZB4YrULCeByFzX45EayG))
- **create_token**: create `tpd-081ea8-token.testnet` with 1.632 NEAR ([tx](https://testnet.nearblocks.io/txns/2PYtg9znzjkc1BjDPTxDqTpUt2FmCFN3a596Uwfxczyt))
- **create_wallet**: create `tpd-081ea8-wallet.testnet` with 1.846 NEAR ([tx](https://testnet.nearblocks.io/txns/HoZkBSb85ddcZMydt8tkUhtPzRH1Mrfnp8ecBHYZM3Nh))
- **deploy_token**: deploy the test token (NEP-141, 6 decimals, emits NEP-297 events) and mint the supply to the owner ([tx](https://testnet.nearblocks.io/txns/9ANeAc3maerDMTP38eqKv85LfyA73rW9FG4dYY7c7wLd))
- **deploy_wallet**: deploy the policy wallet, `new(owner, [token])`, and delete the wallet's own key, in one batch ([tx](https://testnet.nearblocks.io/txns/G5diFS2MXAdQ7oxHJQTKnW2yX2jsPFM7BP83KSXT3gYA))
- **register_storage**: owner registers the wallet and the gateway on the token ([tx](https://testnet.nearblocks.io/txns/8MwABt2UE6oQF3dY7y9CFp4edauSDu6We7LBKCGj79P6))
- **fund_wallet**: owner funds the wallet with 10.000000 tokens ([tx](https://testnet.nearblocks.io/txns/9MJ8PnekqqA3qq9GAChNFaeT76kt382JgRQK6MBBB2k2))
- **set_agent**: owner sets the agent's policy: 1.000000 per call, 4.000000 per UTC day, payees `[tpd-081ea8-gateway.testnet]` ([tx](https://testnet.nearblocks.io/txns/BueB9TmQxskRbEtZBevFFNLruDqApWu6KwgrmpqNRzt8))
- **add_agent_key**: the agent account adds a function-call key: receiver = the wallet, methods = `[pay]` ([tx](https://testnet.nearblocks.io/txns/FeEc6TWEdnoezSri29dm8LkfWiMor6UWaR8zdM4R1VBy))
- **keys_view**: on chain: the wallet has 0 access keys; the agent's key permission is `{"FunctionCall":{"allowance":"500000000000000000000000","method_names":["pay"],"receiver_id":"tpd-081ea8-wallet.testnet"}}`; the wallet holds 10.000000 tokens

Start the API with:

```
APS_PROVIDER=near NEAR_NETWORK=testnet NEAR_GATEWAY_ACCOUNT=tpd-081ea8-gateway.testnet NEAR_ASSET_CONTRACT=tpd-081ea8-token.testnet cargo run -p aps-api --bin aps-api
```

## The agent pays per call

- **scan:1-rounding-drain-placeholder-evidence**: 402, wallet `pay` of 1.000000, proof accepted, verdict **needs_evidence** (missing: deployment_evidence); payer on chain `tpd-081ea8-wallet.testnet`; remaining today 3.000000 ([tx](https://testnet.nearblocks.io/txns/GgrfPPB58SuUGvtAotrS4Fy2y8sB5j3khBs5UjAcprQB))
- **scan:2-rounding-drain-with-evidence**: 402, wallet `pay` of 1.000000, proof accepted, verdict **supported**; payer on chain `tpd-081ea8-wallet.testnet`; remaining today 2.000000 ([tx](https://testnet.nearblocks.io/txns/F2k8QVSBALjNsgHwMkVA5aFmzH2KWNvqdUwLytuH3uyF))
- **scan:3-fee-withdrawal-with-injection**: 402, wallet `pay` of 1.000000, proof accepted, verdict **supported**; the report flags embedded instructions; payer on chain `tpd-081ea8-wallet.testnet`; remaining today 1.000000 ([tx](https://testnet.nearblocks.io/txns/8pU47xtzEuCjaqPnLGw6VqWp5pwCJsE8J1fKBaCqLHM7))
- **scan:4-owner-can-pause**: 402, wallet `pay` of 1.000000, proof accepted, verdict **rejected** (missing: quantified_impact) (failed: privileged_actor); payer on chain `tpd-081ea8-wallet.testnet`; remaining today 0.000000 ([tx](https://testnet.nearblocks.io/txns/yzVYR3S843VobhidDDdYPn1yo5atAjHvKWwfQsFPGDW))

## What the key cannot do

- **injected_payment**: the agent follows the instruction hidden in finding 3 and calls `pay(receiver = tpd-081ea8-mallory.testnet)`: refused with `ERR_PAYEE_NOT_ALLOWED`; wallet balance change 0 ([tx](https://testnet.nearblocks.io/txns/JCCootjaE1TeK67fSTLyvc9ZTWW11k4P6hmDGcxenDFJ))
- **over_per_call**: `pay` of 1.000001 (one atomic unit over the per-call limit): refused with `ERR_PER_CALL_LIMIT`; wallet balance change 0 ([tx](https://testnet.nearblocks.io/txns/AD8MaGVBgj8CksWT2EPpyjhhKBjqBqKJzAXonTKaYoa3))
- **scan:over-daily-limit**: 402 received; the wallet REFUSED the payment with `ERR_DAILY_LIMIT`; wallet balance change 0; no verdict ([tx](https://testnet.nearblocks.io/txns/8Htyd8ho9WGuh5BdsiCD7faHPQdL42pAsPMsLgrgSzTQ))
- **self_escalation**: the agent key calls the owner method `set_agent` with the required 1 yoctoNEAR: the node rejects the transaction before execution (`Deposit is not allowed with function call Deposit is not allowed with function call`)
- **self_escalation_no_deposit**: the same call with no deposit: rejected by the node (`Access key does not allow method set_agent Access key does not allow method set_agent`)

State after the run: spent today 4.000000 of 4.000000; wallet holds 6.000000; gateway holds 4.000000.


## A direct payment verifies the same way

- **direct_payment**: the owner pays with a plain `ft_transfer` (full-access key, 1 yoctoNEAR): proof accepted, verdict **supported**, payer on chain `tpd-081ea8-owner.testnet`; the same proof and request sent again returns the identical response, with no second charge ([tx](https://testnet.nearblocks.io/txns/ADqKwABqUTaKz1wSRNyvDTmd59fLMYPdSskepwDYsA14))
- **proof_reuse**: the same proof with a different request body: HTTP 402 `proof_already_consumed`
## The owner revokes the agent

- **revoke_agent**: owner calls `revoke_agent` (1 yoctoNEAR) ([tx](https://testnet.nearblocks.io/txns/8s1njXjpZPMgyuQ3yCG4SBwD2vcXokjpdpNRppYcCPCq))
- **pay_after_revoke**: the agent's key still exists and calls `pay` for one atomic unit: refused with `ERR_AGENT_INACTIVE`; wallet balance change 0 ([tx](https://testnet.nearblocks.io/txns/6enzy3yGcVXXRyCmDWsrW4qgg6AVfNPpbFGiEATxEShD))
- **delete_agent_key**: the agent account's full-access key deletes the function-call key (the wallet owner cannot do this; revocation at the wallet does not depend on it) ([tx](https://testnet.nearblocks.io/txns/4baXiB8WZsCZ2rzErFq97G7uQvCmCSTYE7k8Nb5PW2q6))
- **pay_with_deleted_key**: `pay` signed with the deleted key: rejected by the node (`Access key ed25519:<key> does not exist at block height 277206457`)
- **ledger_after**: server ledger for the x402 account: deposited 5.000000, settled 5.000000, available 0.000000

Finished 2026-11-11T11:01:45.504Z.

---

## Live checks of the 2026-11-11 fixes

Run at 2026-11-11T11:02:27.967Z against the same server and accounts. A third party must
not be able to make a requirement unfundable; a real payment must still
settle.

- **A failed transaction hinted at a pending requirement.** The failed
  `ERR_DAILY_LIMIT` transaction above ([tx](https://testnet.nearblocks.io/txns/8Htyd8ho9WGuh5BdsiCD7faHPQdL42pAsPMsLgrgSzTQ)) was
  submitted as a hint for a fresh requirement through
  `POST /payment/intents/{id}/observations`: HTTP 200,
  reconciled `rejected`. The requirement stayed open: the
  real payment ([tx](https://testnet.nearblocks.io/txns/Dr4ENt5nod2WRMmhfS9cfCcu4o8ALVBfdB9Vw8gX3Bj5)) then settled it, HTTP
  200, verdict `supported`.
- **A one-unit transfer carrying the memo.** One atomic unit was sent to the
  gateway with a fresh requirement's memo ([tx](https://testnet.nearblocks.io/txns/8PTv63koaaGj5JWAWRa6jRNBp7ZzJqKDxJ1WgsfEgLH6)) and
  presented: HTTP 402 `amount_mismatch`. The exact payment
  ([tx](https://testnet.nearblocks.io/txns/4hargcnme2YDUxh2tgQQvbU7c8MKra5yeciyHJpsNzv9)) then settled the same requirement, HTTP
  200. `GET /payment/intents/{id}` reports status
  `confirmed` and lists the one-unit transfer as an orphan
  (amount `1`, reason "the transferred amount does not match the intent").
- **The direct credit route.** `POST /accounts/freeloader/deposits` with the
  NEAR provider active: HTTP 403 `dev_only_route`.

## What this run does not show

- The Stage B model judge: every verdict above is Stage A (deterministic
  gates) only.
- The sandbox test suite. It had not run when this transcript was
  recorded; it passed on Linux CI the next day (2026-11-12).
- Mainnet: nothing here touched mainnet.
