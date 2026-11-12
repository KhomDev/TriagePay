# Owner approval

`pay` still transfers immediately when review is off. That is the default,
so the existing refusals (`ERR_PAYEE_NOT_ALLOWED`, the limit errors) stay
as they are.

The owner turns review on with `set_review_policy(approval_threshold, review_unknown_payees)`,
attaching 1 yoctoNEAR:

- `review_unknown_payees`: a payee that is not on the allowlist is reserved
  and queued with reason `new_payee`, instead of `ERR_PAYEE_NOT_ALLOWED`.
- `approval_threshold`: an allowlisted amount strictly above this value is
  queued with reason `above_threshold`. Zero disables that rule. An amount
  equal to the threshold still pays.

The per-call cap, the daily cap, the token allowlist, pause, and revocation
still apply before anything is queued. The queued amount counts against the
day. The queue holds 8 payments (`ERR_REVIEW_QUEUE_FULL` leaves the ninth
unreserved).

`approve_payment(id)` schedules the `ft_transfer` the queue skipped. It
refuses while the wallet is paused or the agent is inactive, and it leaves
the row in place so the owner can reject it instead.
`reject_payment(id)` drops the row and restores the reservation when the
UTC day is still the one it was reserved in. Reject works while paused.

`get_review_policy` and `get_pending` are the views. The hosted page reads
them.

## Live on testnet

The hosted demo's wallet is `tpd-e947e7-wallet.testnet`, deployed on
2026-11-12 with this queue. Its policy is `approval_threshold` = the small
demo payment (0.0001 wNEAR) and `review_unknown_payees` = false, so an
injected payee is still refused (`ERR_PAYEE_NOT_ALLOWED`) and an amount
above 0.0001 waits for the owner. On that wallet I ran an allowed payment,
the injected and over-limit refusals, a queued payment the owner approved
and the wallet settled, a second one the owner rejected, revoke and a
revoked-agent refusal. Every transaction is in
[docs/demo/testnet-review/2026-11-12](../demo/testnet-review/2026-11-12/README.md).

The previous demo wallet, `tpd-797021-wallet2.testnet`, does not have the
methods: the account has no access key, so the wasm could not be replaced
in place. The page no longer uses it.

## Deploy

From a machine that holds a funded testnet key and can reach the faucet.
The script creates new accounts.

```sh
cd contracts
RUSTFLAGS='--cfg near' cargo build --release --target wasm32-unknown-unknown -p policy-wallet --locked
cd ..
TRIAGEPAY_WALLET_WASM=contracts/target/wasm32-unknown-unknown/release/policy_wallet.wasm node demo/live/setup.mjs
node demo/live/proof.mjs
```

setup.mjs refuses a wasm that embeds a host path. If the cargo home or the checkout sits under a user directory, add `--remap-path-prefix=<that dir>=/src` for each one together with `--cfg near`. On Windows I passed them in `CARGO_ENCODED_RUSTFLAGS` to a plain `cargo build`; `cargo near build` did not apply them.

`setup.mjs` calls `set_review_policy` with the threshold equal to the small
demo payment. Unknown payees stay refused unless
`TRIAGEPAY_REVIEW_UNKNOWN_PAYEES=1` is set, in which case they are queued
too (and the injected-payee attack is held instead of refused). Copy the
new account ids from the printed `config.json` if they changed, and give
Vercel a pay-only key for the agent and a key for the owner. The secrets
stay out of git and off any command line that gets logged.
`node demo/live/proof.mjs adopt` adds existing hosted public keys
(`HOSTED_AGENT_PUBLIC_KEY`, `HOSTED_OWNER_PUBLIC_KEY`) to the new accounts
instead, and `node demo/live/proof.mjs finish` leaves the agent with only
the hosted pay key.

When the wallet has the queue but both rules are off, the page disables
the queue button and the walkthrough stays a walkthrough.