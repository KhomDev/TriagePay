# ADR-0008: Spend-policy wallet (draft)

| Field | Value |
|---|---|
| Status | Draft (pre-window spike; lands as `docs/decisions/0008-spend-policy-wallet.md`) |
| Date | 2026-10-21 |
| Supersedes | The function-call-key-on-the-asset-contract proposal (corrected under prompt 01) |
| Related | `docs/policy-wallet.md`, `docs/mainnet-deployment-plan.md` |

## Context

An AI agent that pays per call needs money, but holding a full-access key
gives it everything in the account. The settlement asset is a NEP-141 token
(native USDC, 6 decimals). Two platform rules decide the design:

1. A NEAR function-call access key cannot attach a deposit to the calls it
   signs.
2. NEP-141 `ft_transfer` requires exactly 1 yoctoNEAR attached
   (`assert_one_yocto`), precisely so that function-call keys cannot move
   tokens.

So no function-call key on the agent's own account can ever call
`ft_transfer`, and a full-access key cannot be limited.

## Decision

Put the agent's budget in a dedicated wallet contract account that enforces
the spending policy on chain.

- **Owner account (human).** Full access to its own account. Funds the
  wallet, sets policy, pauses, withdraws, revokes agents. Every owner method
  requires exactly 1 yoctoNEAR and `predecessor == owner`.
- **Wallet contract account** (for example `agent-wallet.<owner>`). Holds the
  token balance and a little NEAR for storage and callback gas. After deploy
  and init it has **no access keys**.
- **Agent accounts.** Each holds a function-call key with
  `receiver_id = wallet` and `method_names = ["pay"]`, plus a gas allowance.
  `pay` takes zero deposit, so the key is sufficient. Per-agent policies
  make multi-agent budgets a configuration change.

`pay(token, receiver_id, amount, memo)` checks, in order: not paused, agent
active, token allowlisted, payee allowlisted, amount non-zero, `amount <=
per_call_max`, window total + amount `<= daily_max` (checked u128). It then
reserves the amount, emits `pay_requested`, and calls `ft_transfer` with
1 yoctoNEAR from the wallet's own NEAR balance, followed by the private
`on_pay_complete` callback. The callback emits `pay_settled`, or releases the
reservation and emits `pay_failed`. The window is the UTC day of the block
timestamp.

The policy arithmetic lives in `policy-core`, a dependency-free Rust crate
that the contract wraps and that off-chain simulators and property tests use
directly, so the rule enforced on chain and the rule modelled off chain are
one implementation.

## Alternatives rejected

| Alternative | Why rejected |
|---|---|
| Function-call key on the asset contract, limited to `ft_transfer` | Infeasible: function-call keys cannot attach the 1 yoctoNEAR that `ft_transfer` requires. Even if it could, a key's `allowance` bounds gas spend only, not token amounts, payees or a daily total. |
| Limits enforced only by the off-chain gateway or agent host | The limit is only as strong as the host that holds a full-access key; a compromised host spends everything. |
| Owner co-signs every payment | Removes autonomy, which is the point of an agent wallet. |
| Upgradeable contract (owner-callable `upgrade`) | An upgrade method lets an owner-key compromise replace the policy with code that drains the wallet; it also enlarges the audit surface. See upgrade policy. |

## Upgrade policy

The contract is immutable: there is no upgrade or migrate method, and the
deploy batch deletes the wallet account's only full-access key after `new`.
The upgrade path is to deploy a new wallet to a new account, re-provision
agents, and withdraw the remaining balance from the old wallet with
`withdraw` (which keeps working while paused and for de-listed tokens).

Retiring a wallet does not need an upgrade either. The owner pauses it,
withdraws every token, and calls `close(beneficiary)` (exactly 1 yocto,
owner only, paused only). `close` queries `ft_balance_of(wallet)` on every
allowlisted token; its private callback deletes the wallet account to the
beneficiary only if every balance is zero, and otherwise emits
`close_refused` and changes nothing. Deletion returns the wallet's NEAR,
including the storage stake, so a keyless wallet no longer locks its
stake forever. The code itself is never replaced: there is still no
upgrade, migrate or redeploy path.

## Consequences

- An agent-key compromise is bounded by that agent's `daily_max` per UTC
  day, paid only to that agent's allowlisted payees, plus the key's gas
  allowance (paid from the agent account's NEAR, not the wallet).
- An owner-key compromise loses the wallet balance (the owner can withdraw).
  This is the same trust as holding the funds directly; keep the owner key
  offline.
- `new` is `#[init]` and `#[private]`: deploy and init happen in one batch
  signed by the wallet account itself, so no third party can initialize a
  freshly deployed wallet.
- A failed downstream transfer never strands budget: the callback releases
  the reservation, and the callback itself never panics. `pay` refuses less
  than 35 TGas so the callback always has gas.
- Refusals panic with stable `ERR_*` codes, so nothing changes on chain and
  the reason is visible on any explorer.
- The storage stake is recoverable: an empty, paused wallet is closable by
  the owner, and account deletion returns its NEAR to the beneficiary.
  Tokens held but no longer allowlisted are not checked by `close` and
  must be withdrawn first, and the beneficiary must exist.

## Open items

- Confirm on testnet that recovery of the storage stake by account
  deletion behaves as expected (`close` after withdrawing all tokens).

- Reproducible wasm build (`cargo near build reproducible-wasm`) with a
  pinned image, so the deployed code hash can be re-derived by anyone.
- First sandbox run on Linux CI (the sandbox does not run on Windows).
- Testnet end-to-end with throwaway accounts (prompt 02 work item 6).
