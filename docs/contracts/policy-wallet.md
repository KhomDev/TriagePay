# Policy wallet

A NEAR contract that holds an agent's NEP-141 budget and enforces its
spending policy. Design rationale: `docs/decisions/0008-spend-policy-wallet.md`.
Build, toolchain and wasm hash: `docs/contracts/build.md`.

Source layout: `crates/policy-core` (the dependency-free policy arithmetic,
a member of the root workspace) and the `contracts/` Cargo workspace
(`policy-wallet`, `test-token`, and the near-workspaces sandbox tests in
`contracts/tests`).

Status: unit and property tests pass on the Windows build host; the sandbox
integration tests are written but have only been compiled, not run (the
sandbox has no Windows build; Linux CI runs them). A zero-value testnet
end-to-end run passed all 28 steps on 2026-11-06 (see "Testnet evidence").
Nothing has been deployed to mainnet.

## Interface

All amounts are NEP-141 atomic units as JSON strings (`U128`). Timestamps
and ids are JSON strings (`U64`).

| Method | Caller | Deposit | Prepaid gas (enforced minimum, or a planning value not yet measured) | Effect |
|---|---|---|---|---|
| `new(owner, tokens)` | the wallet account itself (`#[init]`, `#[private]`) | 0 | 30 TGas | Initialize once; `tokens` is the NEP-141 allowlist (max 16) |
| `pay(token, receiver_id, amount, memo?)` | a registered agent (predecessor) | exactly 0 | >= 35 TGas (50 recommended) | Check policy, reserve, `ft_transfer` with 1 yocto, then `on_pay_complete`. Returns the promise |
| `on_pay_complete(agent_id, token, receiver_id, amount, payment_id, window_index)` | the wallet itself (`#[private]`) | - | 10 TGas (static) | `pay_settled`, or release the reservation and `pay_failed`; returns `true` when settled |
| `set_agent(agent_id, per_call_max, daily_max, payees)` | owner | exactly 1 yocto | 10 TGas | Create or reconfigure and activate an agent; an existing agent keeps its spend window |
| `revoke_agent(agent_id)` | owner | exactly 1 yocto | 10 TGas | Deactivate; spend window kept |
| `set_tokens(tokens)` | owner | exactly 1 yocto | 10 TGas | Replace the token allowlist |
| `pause()` / `unpause()` | owner | exactly 1 yocto | 10 TGas | Stop / resume every `pay` |
| `withdraw(token, amount, to)` | owner | exactly 1 yocto | >= 35 TGas | `ft_transfer` any held token to `to` (works while paused) |
| `on_withdraw_complete(token, amount, to)` | the wallet itself (`#[private]`) | - | 10 TGas (static) | `withdrawn` or `withdraw_failed` |
| `close(beneficiary)` | owner, wallet paused | exactly 1 yocto | >= 25 TGas + 5 TGas per allowlisted token (50 recommended) | Query `ft_balance_of(wallet)` on every allowlisted token, then `on_close_check` |
| `on_close_check(beneficiary, tokens)` | the wallet itself (`#[private]`) | - | 15 TGas (static) | All balances zero: emit `closed` and delete the wallet account to `beneficiary`. Otherwise emit `close_refused` and change nothing; returns `false` |
| `ft_on_transfer(sender_id, amount, msg)` | an allowlisted token contract | - | - | Accept funding via `ft_transfer_call` (returns `"0"`); any other token is refused and refunded |
| `get_agent(agent_id)` | anyone (view) | - | - | Limits, payees, active, spent and remaining in the current UTC day, window index, next reset time; `null` if unknown |
| `get_spent(agent_id)` | anyone (view) | - | - | Spent in the current UTC day (`"0"` if unknown) |
| `get_config()` | anyone (view) | - | - | Owner, paused, tokens, next payment id, event standard and version |

`pay` check order (first failure wins): paused, agent active, token
allowlisted, payee allowlisted, non-zero amount, per-call limit, daily limit
(checked add). Deposit, memo length (256 bytes) and prepaid gas are checked
before the policy. The daily window is the UTC day of the block timestamp
(`timestamp_ns / 86_400_000_000_000`).

## Events (NEP-297)

Logged as `EVENT_JSON:{"standard":"aps_policy_wallet","version":"1.0.0","event":...,"data":{...}}`.

| Event | Data |
|---|---|
| `pay_requested` | `payment_id`, `agent_id`, `token`, `receiver_id`, `amount`, `window_index` |
| `pay_settled` | `payment_id`, `agent_id`, `token`, `receiver_id`, `amount` |
| `pay_failed` | as `pay_settled`, plus `release`: `restored`, `window_expired` or `mismatch` |
| `agent_set` | `agent_id`, `per_call_max`, `daily_max`, `payees` |
| `agent_revoked` | `agent_id` |
| `tokens_set` | `tokens` |
| `paused` / `unpaused` | `by` |
| `funded` | `token`, `sender_id`, `amount` |
| `withdrawn` / `withdraw_failed` | `token`, `amount`, `to` |
| `closed` | `beneficiary` (emitted just before the account deletion) |
| `close_refused` | `tokens`: list of `{token, balance}` that blocked the close (`balance` is `null` when the query failed) |

## Error codes

Every refusal panics with `CODE: description`; the code is stable.

| Code | Raised by | Meaning |
|---|---|---|
| `ERR_PAUSED` | `pay` | The owner paused the wallet |
| `ERR_AGENT_INACTIVE` | `pay`, `revoke_agent` | Unknown or revoked agent |
| `ERR_TOKEN_NOT_ALLOWED` | `pay`, `ft_on_transfer` | Token not on the allowlist |
| `ERR_PAYEE_NOT_ALLOWED` | `pay` | Receiver not on the agent's payee list |
| `ERR_ZERO_AMOUNT` | `pay`, `withdraw` | Amount is zero |
| `ERR_PER_CALL_LIMIT` | `pay` | Amount above `per_call_max` |
| `ERR_DAILY_LIMIT` | `pay` | Window total would exceed `daily_max` |
| `ERR_OVERFLOW` | `pay` | u128/u64 arithmetic would overflow |
| `ERR_INVALID_POLICY` | `set_agent` | `per_call_max` is 0, `daily_max < per_call_max`, or more than 32 payees |
| `ERR_RELEASE_MISMATCH` | policy core only | A release larger than the window (the contract logs `mismatch` instead of panicking) |
| `ERR_NOT_OWNER` | owner methods | Predecessor is not the owner |
| `ERR_DEPOSIT_NOT_ZERO` | `pay` | A deposit was attached |
| `ERR_INSUFFICIENT_GAS` | `pay`, `withdraw` | Less than 35 TGas prepaid |
| `ERR_MEMO_TOO_LONG` | `pay` | Memo over 256 bytes |
| `ERR_TOO_MANY_TOKENS` | `new`, `set_tokens` | More than 16 tokens |
| `ERR_SELF_ACCOUNT` | `new`, `set_agent`, `set_tokens`, `withdraw`, `close` | The wallet itself used as owner, agent, payee, token, destination or beneficiary |
| `ERR_NOT_PAUSED` | `close` | Pause the wallet before closing |
| `Requires attached deposit of exactly 1 yoctoNEAR` | owner methods | near-sdk `assert_one_yocto`; what any function-call key hits |

## Threat table

| Threat | Mitigation | Residual risk |
|---|---|---|
| Agent key theft | The key can only call `pay` on the wallet with zero deposit. Spend is capped at `daily_max` per UTC day, to allowlisted payees only. The owner can `revoke_agent` or `pause` at once and delete the key from the agent account | Up to one day's limit to allowlisted payees, plus the key's gas allowance (paid in the agent account's NEAR) |
| Agent tries owner methods | Every owner method requires exactly 1 yocto (function-call keys cannot attach a deposit) and `predecessor == owner` | None known (tested in unit and sandbox suites) |
| Agent forges a callback to refund itself or to delete the wallet | `on_pay_complete`, `on_withdraw_complete` and `on_close_check` are `#[private]` | None known |
| Premature or hostile `close` | Owner only, exactly 1 yocto, paused only; deletes only when every allowlisted balance reads zero; a failed or unreadable balance query refuses | Tokens held but no longer allowlisted are not checked (withdraw them first); a missing beneficiary loses the NEAR |
| Owner key compromise | Owner can withdraw everything; keep the key offline or in a hardware wallet | Full wallet balance |
| Contract bug | Policy math in a pure crate with property tests; checked u128; immutable code; small balances in the wallet | Funds in the wallet at the time |
| Redeploy or re-init by a third party | `new` is private and init-once; the wallet account keeps no access keys after the deploy batch | None while no key exists on the wallet account |
| Callback failure (out of gas) | `pay` refuses less than 35 TGas; the callback never panics | If it did fail, the reservation stays counted (fails closed: budget lost for the day, no tokens lost) |
| Downstream `ft_transfer` fails (receiver not storage-registered, token paused) | Callback releases the reservation and emits `pay_failed` | None; tokens never left |
| Griefing via storage | Only the owner adds agents; `pay` writes fixed-size state; payee and token lists are bounded (32, 16); memos are not stored | Owner-driven storage only |
| Limit-window edge cases | UTC day from block timestamp; last nanosecond of a day is the same window; a release after midnight is a no-op (`window_expired`) instead of refunding the new day; a backwards clock keeps counting instead of resetting; revoke and re-add keeps the window | A burst across midnight can spend up to 2 x `daily_max` within minutes (one full limit per calendar day) |
| Concurrent pays | NEAR executes one receipt at a time per account; each `pay` sees the previous reservation | None |
| Non-standard token behaviour | Only allowlisted tokens are accepted or paid | Depends on the allowlisted token contracts |

## Threat model rows

Proposed rows for `docs/threat-model.md`. Each row reads threat, then
mitigation, then the test that exercises it, then the residual risk. Test
names refer to `crates/policy-core` (unit and property), the
`policy-wallet` unit suite (`contracts/policy-wallet/src/tests.rs`), the
sandbox suite (`contracts/tests/sandbox.rs`, Linux CI only, not yet
executed), and the numbered testnet steps above.

| Threat | Mitigation | Test | Residual risk |
|---|---|---|---|
| Agent key stolen | The key is a function-call key: receiver = wallet, methods = `pay`, zero deposit. `pay` enforces per-call and daily limits and the payee allowlist; the owner can `revoke_agent` or `pause` at once | Unit `pay_over_per_call_limit`, `pay_over_daily_limit`, `pay_to_non_allowlisted_payee`, `pay_by_revoked_agent`; sandbox `per_call_limit_is_refused_and_nothing_moves`, `daily_limit_is_refused_and_nothing_moves`, `non_allowlisted_payee_is_refused`, `revoked_agent_is_refused`; testnet steps 13, 14, 22, 24 | Up to `daily_max` per UTC day, paid only to allowlisted payees (up to 2 x `daily_max` across a midnight), plus the key's gas allowance, paid in the agent account's NEAR |
| Agent key used on owner methods | Every owner method requires exactly 1 yoctoNEAR, which no function-call key can attach, and `predecessor == owner` | Unit `owner_with_two_yocto_is_refused`; sandbox `agent_function_call_key_cannot_call_owner_methods`, `agent_full_access_key_is_not_the_owner`; testnet steps 15 (node rejection), 16 (`ERR_NOT_OWNER`) | None known |
| An owner function-call key (for example a dapp login key for the wallet, any method) used on owner methods | Same 1-yocto rule: with zero deposit the contract refuses, with 1 yocto the node refuses | Unit `close_requires_one_yocto_from_owner`; testnet steps 18 (`assert_one_yocto`), 19 (node rejection) | None known |
| Owner key stolen | None on chain: the owner can withdraw everything. Keep the owner key offline; fund the wallet with a small working balance | Sandbox `owner_withdraws_tokens`; testnet step 27 (shows the power) | Full wallet balance |
| Bug in the policy arithmetic | Pure `policy-core` crate shared by contract and simulator; checked u128 math; `overflow-checks = true` in the release profile; property tests | Properties `spend_never_exceeds_daily_max`, `reserve_then_release_restores_exact_state`, `rollover_resets_correctly`; unit `pay_overflow_adjacent_amount`; sandbox `overflow_adjacent_amount_uses_checked_math` | Funds in the wallet at the time; the code is immutable, so a fix means redeploying to a new account |
| Third party initializes or redeploys the wallet | `new` is `#[init]` and `#[private]`; deploy, `new` and `DeleteKey` run in one batch; no upgrade method | Sandbox `init_and_callbacks_are_private`; testnet step 5 (zero access keys, code hash matches) | None while the account has no access key |
| Forged callback (refund itself, delete the wallet) | `on_pay_complete`, `on_withdraw_complete`, `on_close_check` are `#[private]` | Sandbox `init_and_callbacks_are_private` | None known |
| Downstream `ft_transfer` fails (receiver not registered, token paused) | The callback releases the reservation and emits `pay_failed`; a release after UTC midnight is a no-op | Unit `callback_failure_releases_the_reservation`, `callback_failure_after_rollover_does_not_touch_new_window`; sandbox `unregistered_receiver_failure_releases_the_reservation` | None; tokens never left the wallet |
| Callback runs out of gas | `pay` and `withdraw` refuse less than 35 TGas prepaid; the callback never panics | Unit `pay_with_too_little_gas` | Fails closed: if a callback still failed, the reservation stays counted (budget lost for the day, no tokens lost) |
| Concurrent pays in one block | NEAR executes one receipt at a time per account; each `pay` sees the previous reservation | Unit `two_pays_in_the_same_block_both_count`; sandbox `concurrent_pays_cannot_exceed_the_daily_limit` | None |
| Limit-window edge at UTC midnight | Window = UTC day of the block timestamp; a backwards clock keeps counting; revoke and re-add keeps the window | Unit `window_rolls_over_at_utc_midnight`, `set_agent_reconfigure_keeps_the_window`; sandbox `window_rolls_over_after_fast_forward`; property `rollover_resets_correctly` | Up to 2 x `daily_max` within minutes across midnight |
| Premature or hostile `close` | Owner only, exactly 1 yocto, paused only; deletes only when every allowlisted token balance reads zero; a failed or unreadable query refuses | Unit `close_requires_paused`, `close_check_with_a_nonzero_balance_refuses`, `close_check_with_a_failed_query_refuses`; sandbox `close_is_owner_only_and_requires_paused`, `close_is_refused_while_tokens_remain`; testnet step 26 | Tokens held but no longer allowlisted are not checked (withdraw them first); a non-existent beneficiary loses the NEAR |
| Storage stake locked forever in a keyless wallet | `close` deletes the account and returns its whole NEAR balance to the beneficiary | Unit `close_check_with_all_zero_balances_deletes_the_account`; sandbox `close_after_withdrawing_everything_deletes_the_wallet`; testnet step 28 (1.74951 NEAR stake returned) | The wallet's NEP-145 storage deposit on each token (0.00125 NEAR for USDC) stays with the token contract |
| Storage griefing | Only the owner adds agents; payee and token lists are bounded (32, 16); memos are not stored | Unit `set_agent_rejects_too_many_payees`, `new_rejects_too_many_tokens`, `pay_with_long_memo` | Owner-driven storage only |
| Non-standard or hostile token | Only allowlisted tokens are accepted (`ft_on_transfer`) or paid | Unit `ft_on_transfer_refuses_other_tokens`, `pay_with_disallowed_token`; sandbox `disallowed_token_is_refused` | Depends on the allowlisted token contract; native USDC's account holds a full-access key, so its issuer can upgrade it |
| Build-host details published with the code | The wasm is built with `--remap-path-prefix` for the cargo home, rustup home and checkout; CI fails if the runner's home path survives; both scripts refuse a wasm containing a host path | `docs/contracts/build.md` string scan (zero hits); the CI step "Print wasm size and SHA-256, and check for host paths"; testnet step 5 deployed the remapped build | The build is not reproducible yet, so a third party cannot re-derive the deployed hash from source |

## Provisioning walkthrough (plan)

The exact mainnet plan, with costs, is `docs/contracts/mainnet-deployment-plan.md`;
`python scripts/wallet-provision.py` prints it from the built wasm (it never
signs on mainnet). In order:

1. Create the wallet account with one temporary full-access key.
2. One batch signed by that key: deploy the wasm, call `new(owner, tokens)`,
   delete the key. The wallet now has no access keys. (A batch deleting its
   own signing key as its last action was confirmed on testnet, step 5
   below.)
3. Owner: `storage_deposit` on each token for the wallet (and for any payee
   not yet registered).
4. Owner: fund with `ft_transfer_call(receiver_id = wallet, msg = "")`.
5. Owner: `set_agent(agent, per_call_max, daily_max, payees)` with 1 yocto.
6. Agent account: add a function-call key, `receiver_id = wallet`,
   `method_names = ["pay"]`, with a gas allowance.
7. Agent: `pay(token, receiver_id, amount, memo)` with 50 TGas, no deposit.
8. Retire: owner `pause`, `withdraw` every token, then `close(beneficiary)`;
   the account deletion returns the wallet's NEAR, storage stake included.

## Tests

| Suite | Count | Where it runs |
|---|---|---|
| `policy-core` unit | 25 | any host |
| `policy-core` property (proptest) | 3 | any host |
| `policy-wallet` unit (near-sdk mocked runtime) | 62 | any host |
| `test-token` unit | 3 | any host |
| Sandbox integration (near-workspaces) | 18 | Linux CI only; written, not yet executed |
| Testnet end-to-end (`scripts/testnet/wallet-e2e.mjs`) | 28 steps | testnet, 2026-11-06; all passed |

## Testnet evidence (2026-11-06)

A zero-value run on NEAR testnet with throwaway accounts, signed by
`scripts/testnet/wallet-e2e.mjs` (near-api-js 7.3.1) through
`python scripts/wallet-provision.py --network testnet --execute`. Keys
stayed outside the repository; the full evidence file (balances, events,
gas) is kept off-repo. Every step carries an expectation that the script
checked; all 28 passed.

| Item | Value |
|---|---|
| Wasm | 174,583 bytes, SHA-256 `7a599a50b37fc25961da3e0e2c97a9108eb483ef1b591d1506e08e44bf13a6a2` (base58 `9Ebz6ABpWwNkjgst6YoZ48yvkYcbcowRYiBK186Lt9RB`), the build in `docs/contracts/build.md` |
| Accounts | owner `apsw-27c724-owner.testnet`, agent `apsw-27c724-agent.testnet`, payee `apsw-27c724-payee.testnet`, wallet `apsw-27c724-wallet.testnet` (deleted by step 28) |
| Token | `wrap.testnet` (wNEAR, 24 decimals); policy per-call 0.001, daily 0.003, payees `[payee]` |
| Funding | official testnet faucet (5 NEAR to an implicit funder account); the 2026-10-21 attempt had been rate-limited |
| RPC | `https://test.rpc.fastnear.com` (`rpc.testnet.near.org` is deprecated and answered HTTP 429; steps 1-19 completed on it before the limit, steps 20-28 ran on FASTNEAR) |

Transactions (explorer: `https://testnet.nearblocks.io/txns/<hash>`):

| # | Step | Signer key | Tx hash | Expected | Observed |
|---|---|---|---|---|---|
| 1 | Create owner | funder | `5PFitFwHYej7WhvEAb1uCQvtTTM6JKp2D2PsXukTfote` | success | success |
| 2 | Create agent | funder | `G7TZjeg2Bt4h6ymiTQ38zRApSr6ApnwYUjfU88u1Ri7N` | success | success |
| 3 | Create payee | funder | `BdHPwxkjokTXwoHbwRnvsHAaoTBLRhURssaaPnGSyXFd` | success | success |
| 4 | Create wallet (stake + 0.1 NEAR) | funder | `DM7mRLzyKW9XkBSg3TLvMQCd3WeixCu9xa95LjVd2ktC` | success | success |
| 5 | Deploy + `new` + delete its own key, one batch | wallet (temporary full access) | `7921vAGsGs38jJsueBAQ7oLCcHMnyJ1NG5oJZNqpVBHp` | zero keys left, code hash = wasm hash | 0 access keys; code hash `9Ebz6ABp...` matches; `get_config` shows the owner and `wrap.testnet` |
| 6 | Owner registers on `wrap.testnet` and wraps 0.05 NEAR | owner | `6Jpkum6xGCndcTfJn66KWjTMBc3uQef6A3a9HdYmGzT6` | success | success |
| 7 | `storage_deposit` for the wallet | owner | `6xo2fXoc9bbhkmG4zxjHSySwGzps5W8bfuwNE2X1bVj6` | success | success |
| 8 | `storage_deposit` for the payee | owner | `FBkFdnVVwjarM3TUWEVnjLsR7jmwDQW9A5kLYgK1LukX` | success | success |
| 9 | Fund 0.01 wNEAR via `ft_transfer_call` | owner | `A9cP4Q7QciFDxnrn4oNyGa758LBx9ms2XD3ULc2P4snJ` | `funded`; wallet holds 0.01 | `funded`; 0.01 |
| 10 | `set_agent` | owner | `HijhV6qw3hr36VFKQZ8NhKkfojVtt5MxyaG7Ah7pfUdG` | `agent_set` | `agent_set` |
| 11 | Add the agent's function-call key (`pay` only, 0.25 NEAR allowance) | agent (full access) | `Pkk29Uen8FRbyYU34XaBjhoWzGeoRAEutFsEaBHCwYv` | success | success |
| 12 | Pay 0.001 within limits | agent function-call key | `A1ieTUo78rAq8Ycmktd9YwmnmYimHnby5PqneKspenVe` | `pay_settled`; payee +0.001 | `pay_requested`, `pay_settled`; payee delta exactly 10^21 atomic units |
| 13 | Pay 0.001 + 1 atomic unit | agent function-call key | `7696qZ8xHEK6My8giw4CKEbWX5CiQXtwJdeAq8KJnEXQ` | `ERR_PER_CALL_LIMIT`, no balance moves | `ERR_PER_CALL_LIMIT`; wallet balance unchanged |
| 14 | Pay a non-allowlisted account | agent function-call key | `5ctCc9xq6eQMMVmYXoDCkLrzaEcqwnWMH8zevR5LTTTT` | `ERR_PAYEE_NOT_ALLOWED`, no balance moves | `ERR_PAYEE_NOT_ALLOWED`; unchanged |
| 15 | `set_agent` with 1 yocto | agent function-call key | none (rejected before execution) | node rejection | "Deposit is not allowed with function call" |
| 16 | `set_agent` with 1 yocto | agent (full access) | `3t7kFxPuroXSyMyqA5com6AitYdVxsoqD7G3JFHnFLRQ` | `ERR_NOT_OWNER` | `ERR_NOT_OWNER` |
| 17 | Owner adds a function-call key for the wallet, any method | owner | `CBahYkwq4zZPipRGf1gAEB3PVkBTUAmgAv1ZpFjbrLdP` | success | success |
| 18 | `pause` with zero deposit | owner function-call key | `HXwaHgjKfHgN3zN6Lcw1FoQufoj9CJuEUSQgNDz7ec3n` | 1-yocto assertion | "Requires attached deposit of exactly 1 yoctoNEAR" |
| 19 | `pause` with 1 yocto | owner function-call key | none (rejected before execution) | node rejection | "Deposit is not allowed with function call" |
| 20 | Delete the owner's function-call key | owner | `9f8PZ4GPP1LVuTY1rRDSmGrfYzWtGBnXBbGrTCLZLz4L` | success | success |
| 21 | `revoke_agent` | owner | `A5tiKRPwMtWwTWWqcaPp3hAfz92Jmggqk6V1u72jFiHK` | `agent_revoked` | `agent_revoked` |
| 22 | Pay after revoke (key still present) | agent function-call key | `5hTceNecd8wtkTSnqNVyRRb39rVx9rqjbXDJjzdxRa5p` | `ERR_AGENT_INACTIVE`, no balance moves | `ERR_AGENT_INACTIVE`; unchanged |
| 23 | Delete the agent's function-call key | agent (full access) | `8kSrfdvnNSkiCQVphxxFLKVzjufJWQ5DC85S4Cd9wfkp` | success | success |
| 24 | Pay with the deleted key | deleted key | none (rejected before execution) | access key not found | "Access key ... does not exist" |
| 25 | `pause` | owner | `25wipgJoCAWtDjgmsYP986DpmGp8G1r6ZnkyH5ryF9SH` | `paused` | `paused` |
| 26 | `close` while 0.009 wNEAR remains | owner | `8LDKyHziGHsAfr5z5psxHjYBpRaax43NqqihqG2KBngm` | `close_refused`, wallet kept | `close_refused`; wallet exists |
| 27 | `withdraw` the full 0.009 wNEAR to the owner | owner | `HrUt851j8dGffzt1WBfxNPv6dXJMeZpv7g2HQZMNvo6F` | `withdrawn`; wallet wNEAR 0 | `withdrawn`; 0 |
| 28 | `close(owner)` | owner | `8KiGDgPDFmr2wjMXvkhr6xXb3ifqbgDMijsNeHcNxb93` | `closed`; account deleted; owner gains at least the stake | see below |

Storage recovery (step 28). Before the close the wallet held 1.844369 NEAR
and used 174,951 bytes, a storage stake of 1.74951 NEAR at 1e19 yocto per
byte. After it, `view_account` reports the wallet missing, and the owner's
balance rose by 1.843813 NEAR: the wallet's whole balance returned by the
deletion (the stake plus the unused headroom) minus about 0.00056 NEAR of
gas for the close transaction itself. Deleting a 175 KB contract account
worked; no large-state deletion limit applied.
