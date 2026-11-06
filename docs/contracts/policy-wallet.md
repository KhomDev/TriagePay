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
sandbox has no Windows build; Linux CI runs them). Nothing has been
deployed to mainnet.

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

## Provisioning walkthrough (plan)

The exact mainnet plan, with costs, is `docs/contracts/mainnet-deployment-plan.md`.
In order:

1. Create the wallet account with one temporary full-access key.
2. One batch signed by that key: deploy the wasm, call `new(owner, tokens)`,
   delete the key. The wallet now has no access keys. (Deleting the signing
   key as the last action of its own batch is to be confirmed on testnet
   before mainnet.)
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
