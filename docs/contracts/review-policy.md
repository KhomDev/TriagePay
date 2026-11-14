# Owner approval

`pay` still transfers immediately when review is off. That is the default,
so the existing refusals (`ERR_PAYEE_NOT_ALLOWED`, the limit errors) stay
as they are.

The owner turns review on with `set_review_policy(approval_threshold, review_unknown_payees)`,
attaching 1 yoctoNEAR:

- `review_unknown_payees`: a payee that is not on the allowlist is queued
  with reason `new_payee`, instead of `ERR_PAYEE_NOT_ALLOWED`.
- `approval_threshold`: an allowlisted amount strictly above this value is
  queued with reason `above_threshold`. Zero disables that rule. An amount
  equal to the threshold still pays.

The per-call cap, the daily cap, the token allowlist, pause, and revocation
are checked before anything is queued. A hold is checked, not reserved: it
does not count against the day. The queue holds 8 payments
(`ERR_REVIEW_QUEUE_FULL`), one agent may have 3 open holds and open 6 per
UTC day (`ERR_AGENT_OPEN_HOLDS`, `ERR_AGENT_DAILY_HOLDS`), and a hold
expires after 24 hours (`expire_holds`, callable by anyone, drops expired
rows; `pay` and `approve_payment` sweep them too).

`approve_payment(id)` checks the caps again, reserves the amount, and
schedules the `ft_transfer` the queue skipped. It refuses while the wallet
is paused, the agent is inactive, or the caps no longer allow the amount,
and it leaves the row in place so the owner can reject it instead.
`reject_payment(id)`, `reject_payments(ids)` and `reject_all()` drop rows;
nothing was reserved, so the agent's spend does not change. Reject works
while paused.

`get_review_policy` and `get_pending` are the views. The hosted page reads
them.

Builds differ here. The mainnet review-queue wallet `tp-wallet.hlpok1lad.tg`
runs the build from `ef7472c`, where a hold reserved its amount against the
day and a reject restored it. That is how the 2026-11-13 campaign could fill
the day with junk holds. Every build from `94e62f1` on (the factory's
wallets and this source) checks without reserving. This page described the
older rule until the Trail of Bits skill run of 2026-11-14 (C-SC-2).

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