# TriagePay on NEAR mainnet: transcript

NEAR mainnet, real value (tiny wNEAR amounts). RPC https://free.rpc.fastnear.com. Started 2026-11-12T20:48:36.023Z. Owner `hlpok1lad.tg` starts with 2.627341 NEAR.

## Setup

- **create_wallet**: create `tp-wallet.hlpok1lad.tg` with 1.765 NEAR and a temporary deploy key ([tx](https://nearblocks.io/txns/GXQ9hSsnWX6DrqxTfgoLCQW1vQzkMMtf4WPuaB34wLX3))
- **deploy_wallet**: FAILED: deploy the policy wallet (174803 bytes), `new(owner = hlpok1lad.tg, tokens = [wrap.near])`, and delete the wallet's only key, in one batch ([tx](https://nearblocks.io/txns/3XJHm2D329zGHFie2GwouKTaJDFKDZ3tknZHLXM7PE5b))
  - the batch reverted with `LackBalanceForState` (short by 0.027583 NEAR); the code was not deployed and the deploy key stayed; topped up and retried below
- **top_up_wallet**: send 0.045 NEAR more to `tp-wallet.hlpok1lad.tg` for code plus state storage ([tx](https://nearblocks.io/txns/G7RkccZNjEso4uxUAfi2x6iGpW5qzYjiwtdVx7EkkazV))
- **deploy_wallet**: deploy the policy wallet (174803 bytes), `new(owner = hlpok1lad.tg, tokens = [wrap.near])`, and delete the wallet's only key, in one batch ([tx](https://nearblocks.io/txns/AXan6pvjF3sFUtRYbKHAXqabKYrFsy3CEmvojF18Jqfn))
- **create_agent**: create `tp-agent.hlpok1lad.tg` with 0.05 NEAR and exactly one key: a function-call key, receiver = the wallet, methods = `[pay]`, allowance 0.04 NEAR (no full-access key) ([tx](https://nearblocks.io/txns/C9ua1ezirZiXDzxw4Dar9AeBucp1ahTrr2j4iQGmvhDu))
- **wrap_and_register**: owner wraps 0.002 NEAR into wNEAR and registers the wallet and the payee on `wrap.near` ([tx](https://nearblocks.io/txns/DnTfeQQEBH5yTwXDHK5jNAQTwGK4XRMB8Z7qKpaULFqN))
- **fund_wallet**: owner funds the wallet with 0.001 wNEAR (`ft_transfer_call`, accepted by `ft_on_transfer`) ([tx](https://nearblocks.io/txns/Ef1iGZ1FBVNxn21Tf4JYwLknYYKsf9hN3xXhz3LFTun2))
- **set_agent**: owner sets the agent's policy: 1000000 yocto-wNEAR per call, 4000000 per UTC day, payees `[gateway.hlpok1lad.tg]` ([tx](https://nearblocks.io/txns/FrJxyT7KaDsgB2bAeKEQJpXBExUzYCrGCaVn1SNDwf46))
- **keys_view**: on chain: the wallet has 0 access keys; the agent has 1 key(s), permission `[{"FunctionCall":{"allowance":"40000000000000000000000","method_names":["pay"],"receiver_id":"tp-wallet.hlpok1lad.tg"}}]`; the wallet holds 1000000000000000000000 yocto-wNEAR

- **first agent retired**: the first `pay` (50 TGas) from `tp-agent.hlpok1lad.tg` was refused by the node before it was sent (NotEnoughBalance: cost 0.050920 NEAR at the pessimistic gas price, balance 0.05, key allowance 0.04). The agent has no full-access key, so its key cannot be changed: I retired it and created `tp-agent2` with 0.07 NEAR and a 0.15 NEAR allowance, paying with 40 TGas.

- **create_agent**: create `tp-agent2.hlpok1lad.tg` with 0.07 NEAR and exactly one key: a function-call key, receiver = the wallet, methods = `[pay]`, allowance 0.15 NEAR (no full-access key) ([tx](https://nearblocks.io/txns/APN65xR3a2J9UgQ9fRSBnJZ4P8Nx4QdLMeZLgpWMj6Vh))
- **set_agent**: owner sets the agent's policy: 1000000 yocto-wNEAR per call, 4000000 per UTC day, payees `[gateway.hlpok1lad.tg]` ([tx](https://nearblocks.io/txns/76EibxbULaWnfdtVD2rq5q15QSM7M3WWe3NzjfPQA65s))
- **keys_view**: on chain: the wallet has 0 access keys; the agent has 1 key(s), permission `[{"FunctionCall":{"allowance":"150000000000000000000000","method_names":["pay"],"receiver_id":"tp-wallet.hlpok1lad.tg"}}]`; the wallet holds 1000000000000000000000 yocto-wNEAR

## The agent pays

- **paid_call**: the agent's pay-only key calls `pay(wrap.near, gateway.hlpok1lad.tg, 1000000)`: success, events ["aps_policy_wallet:pay_requested","aps_policy_wallet:pay_settled"]; wallet -1000000, payee +1000000 yocto-wNEAR ([tx](https://nearblocks.io/txns/2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ))

## What the key cannot do

- **refused_payee**: a prompt-injected instruction makes the agent call `pay(receiver = tp-mallory.hlpok1lad.tg)`: refused with `ERR_PAYEE_NOT_ALLOWED`; wallet balance change 0 ([tx](https://nearblocks.io/txns/HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F))

## The owner revokes the agent

- **revoke_retired_agent**: owner revokes the retired first agent `tp-agent.hlpok1lad.tg` (its key allowance was too small for one call) ([tx](https://nearblocks.io/txns/6LpSznrmVSBEzyCS51xr8C5a2k2LN6VUKvSvmX8DGSf8))
- **revoke_agent**: owner calls `revoke_agent(tp-agent2.hlpok1lad.tg)` (1 yoctoNEAR) ([tx](https://nearblocks.io/txns/CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh))
- **pay_after_revoke**: the revoked agent's key still exists and calls `pay` to the allowlisted payee for 1 yocto-wNEAR: refused with `ERR_AGENT_INACTIVE`; wallet balance change 0 ([tx](https://nearblocks.io/txns/C1BnCCrEMYfxhirf283GfGr2sfeBMaZ88fimxQwoKt2d))

State: owner 0.669622 NEAR (down 1.957719: 1.810 NEAR in the wallet account, 0.05 in the retired `tp-agent` and 0.07 in `tp-agent2`, 0.002 wrapped, 0.0025 of wNEAR storage, about 0.023 of gas); wallet 1.807076 NEAR, 999999999999999000000 yocto-wNEAR, 0 access keys; payee 1000000 yocto-wNEAR; agent active: false.

