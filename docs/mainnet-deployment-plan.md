# Policy wallet: mainnet deployment plan

**PLAN ONLY. Nothing in this document has been executed.** Every
transaction below is a mainnet write and runs only after the operator
approves that exact plan (signer, receiver, actions, arguments, deposit,
gas, maximum cost) in the session that executes it (standing rule S2;
execution belongs to prompt 09). No key material appears here.

## Inputs

| Item | Value | Source |
|---|---|---|
| Wallet account | `agent-wallet.<operator>` | prompt 02 |
| Owner account | `<operator>` | public operator account (name per the ADR-0006 naming decision) |
| Agent account | `<agent-account>` (an existing or new account of the operator, distinct from the gateway) | operator choice |
| Payee allowlist | `[<gateway>]` only | prompt 02 |
| Token | native USDC `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`, 6 decimals | prompt 02 |
| Policy | `per_call_max` = 0.05 USDC = `"50000"`; `daily_max` = 0.20 USDC = `"200000"` | prompt 02 |
| Wasm | `policy_wallet.wasm`, 165,025 bytes, SHA-256 `0e1b5272d3c03f24f3272d19fddc46751ef7aab9449a0b1295e3798ba38408fd` (spike build, see below) | `NOTES.md` |
| `storage_amount_per_byte` | `10000000000000000000` yoctoNEAR (1e19; 1 NEAR per 100 KB) | read-only `EXPERIMENTAL_protocol_config` on `https://rpc.mainnet.near.org`, finality `final`, 2026-10-21T19:15Z; protocol version 86. Matches the lead's independent read and the research note `docs/research/near-contract-toolchain.md` |
| Gas price | `100000000` yoctoNEAR per gas (0.0001 NEAR per TGas) | read-only `gas_price` on the same endpoint, same time |
| Prepaid-gas planning price | 0.001 NEAR per TGas (10x the burn price; unused gas is refunded) | research note `docs/research/near-contract-toolchain.md` |
| USDC `storage_balance_bounds.min` | 0.00125 NEAR | research note; re-read with a view call at execution time |
| Account creation charge | about 0.007 NEAR | research note |

The wasm hash above is the spike's non-reproducible host build. Before
execution, rebuild with the reproducible flow (pinned cargo-near image) and
replace the size and hash here; the stake and gas figures scale with the
size.

## Storage stake

| Component | Bytes | Stake (NEAR) |
|---|---|---|
| Contract code | 165,025 | 1.65025 |
| Account record (`num_bytes_account`) | 100 | 0.00100 |
| Contract state after `new` and one `set_agent` (estimate: config, token list, one agent entry with one payee, record overheads) | about 500 | about 0.00500 |
| **Total** | **about 165,625** | **about 1.65625** |

Computation: stake = bytes x 1e19 yoctoNEAR = bytes x 0.00001 NEAR.

The wallet keeps no access keys after deployment, so its NEAR can never be
withdrawn and the account can never be deleted: the stake is a permanent
cost. Fund it with the stake plus a small headroom only.

## Transactions

Gas figures use the live fee config read on 2026-10-21 (deploy cost per
byte: 6,812,999 send plus 64,572,944 execution gas). "Max cost" prices all
attached and burnt gas at the 0.001 NEAR/TGas planning price; the expected
cost at the current gas price is one tenth of that.

### T1. Create the wallet account

| Field | Value |
|---|---|
| Signer | `<operator>` (full-access key) |
| Receiver | `agent-wallet.<operator>` |
| Actions | `CreateAccount`; `Transfer` 1.70 NEAR; `AddKey` (temporary full-access key `<K_deploy>`, generated for this run, held outside the repository) |
| Gas | about 8.4 TGas burnt |
| Max cost | 1.70 NEAR transfer + about 0.0084 NEAR gas |

1.70 NEAR covers the 1.65625 NEAR stake with about 0.044 NEAR headroom
(about 4.4 KB of further state, roughly 15 more agent entries).

### T2. Deploy, initialize, lock

| Field | Value |
|---|---|
| Signer | `agent-wallet.<operator>` with `<K_deploy>` |
| Receiver | `agent-wallet.<operator>` |
| Actions | `DeployContract` (the wasm above); `FunctionCall new {"owner": "<operator>", "tokens": ["17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"]}`, deposit 0, gas 30 TGas; `DeleteKey <K_deploy>` |
| Gas | deploy about 11.8 TGas (165,025 x 71,385,943) + 0.4 TGas base; `new` 30 TGas prepaid; delete key 0.2 TGas |
| Max cost | about 0.043 NEAR |

`new` is `#[private]`, so it must be called by the wallet account itself in
this batch. After T2 the account has no access keys and the code cannot be
replaced. Confirm on testnet first that a batch may delete its own signing
key as its last action. Verify after T2: `view_code` hash equals the wasm
hash, `view_access_key_list` is empty, `get_config` shows the owner and the
USDC token.

### T3. Register the wallet on USDC

| Field | Value |
|---|---|
| Signer | `<operator>` |
| Receiver | `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1` |
| Actions | `FunctionCall storage_deposit {"account_id": "agent-wallet.<operator>", "registration_only": true}` |
| Deposit | 0.00125 NEAR (the live `storage_balance_bounds.min`; excess is refunded) |
| Gas | 30 TGas |
| Max cost | 0.00125 NEAR + 0.03 NEAR gas |

Check `storage_balance_of(<gateway>)` first; the gateway already receives
USDC, so it should be registered. If it is not, repeat T3 for `<gateway>`.

### T4. Fund the wallet

| Field | Value |
|---|---|
| Signer | `<operator>` |
| Receiver | USDC contract |
| Actions | `FunctionCall ft_transfer_call {"receiver_id": "agent-wallet.<operator>", "amount": "<funding>", "msg": ""}` |
| Deposit | 1 yoctoNEAR |
| Gas | 100 TGas |
| Max cost | `<funding>` USDC (operator choice; for example `"500000"` = 0.50 USDC, two and a half days of the daily limit) + 0.1 NEAR gas |

The wallet's `ft_on_transfer` accepts the full amount and emits `funded`.

### T5. Register the agent

| Field | Value |
|---|---|
| Signer | `<operator>` |
| Receiver | `agent-wallet.<operator>` |
| Actions | `FunctionCall set_agent {"agent_id": "<agent-account>", "per_call_max": "50000", "daily_max": "200000", "payees": ["<gateway>"]}` |
| Deposit | 1 yoctoNEAR |
| Gas | 30 TGas |
| Max cost | 0.03 NEAR gas (the agent entry's storage, about 0.002 NEAR, comes from the wallet's headroom) |

### T6. Give the agent a pay-only key

| Field | Value |
|---|---|
| Signer | `<agent-account>` (full-access key) |
| Receiver | `<agent-account>` |
| Actions | `AddKey` (new key `<K_agent>`, function-call permission: `receiver_id = agent-wallet.<operator>`, `method_names = ["pay"]`, `allowance = 0.5 NEAR`) |
| Gas | about 0.5 TGas |
| Max cost | about 0.0005 NEAR |

Allowance sizing. A `pay` call is sent with 50 TGas prepaid (the contract
refuses less than 35 TGas: 15 TGas for `ft_transfer`, 10 TGas for the
callback, the rest for `pay` itself). At the 0.001 NEAR/TGas planning price
one call can draw about 0.0503 NEAR from the allowance (50 TGas prepaid
plus about 0.3 TGas of transaction and receipt fees). The daily limit allows
four maximum-size payments, about 0.20 NEAR of allowance per day. An
allowance of 0.5 NEAR covers about nine calls, including refused ones,
before the key must be re-issued, and caps the gas a stolen key can burn at
0.5 NEAR. The agent account must hold at least the allowance in NEAR, since
gas is paid from its balance.

### T7 (first use, prompt 09). Agent pays

| Field | Value |
|---|---|
| Signer | `<agent-account>` with `<K_agent>` |
| Receiver | `agent-wallet.<operator>` |
| Actions | `FunctionCall pay {"token": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1", "receiver_id": "<gateway>", "amount": "50000", "memo": "<job id>"}` |
| Deposit | 0 |
| Gas | 50 TGas |
| Max cost | 0.05 USDC from the wallet + about 0.0503 NEAR of allowance (expected about 0.002 NEAR burnt) |

## Totals (maximum, before refunds)

| Item | NEAR | USDC |
|---|---|---|
| Wallet account balance (T1; permanently locked as stake and headroom) | 1.70 | - |
| Gas, T1 to T6 | about 0.21 | - |
| USDC storage deposit (T3) | 0.00125 | - |
| Funding (T4) | - | `<funding>` |
| Agent account NEAR for the allowance (T6) | 0.5 (spent only as gas) | - |
| **Total** | **about 2.41** | **`<funding>`** |

## Open questions for prompt 09

- Final account names (`<operator>`, `<gateway>`, `<agent-account>`) per
  the ADR-0006 naming decision.
- The reproducible-build hash and size that replace the spike values.
- Whether the operator accepts a permanently keyless wallet (about
  1.70 NEAR locked) or prefers to keep a recovery key and give up the
  immutability guarantee.
- Funding amount and campaign length.
- Testnet confirmation of T2's self-deleting batch.
