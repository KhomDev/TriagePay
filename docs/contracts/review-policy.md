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
them. The wallet on testnet at the time of this change does not have the
methods yet: the account has no access key, so the wasm cannot be replaced
in place.

## Deploy

From a machine that holds a funded testnet key and can reach the faucet.
The script creates new accounts. It does not touch
`tpd-797021-wallet2.testnet`.

```sh
cd contracts
RUSTFLAGS='--cfg near' cargo build --release --target wasm32-unknown-unknown -p policy-wallet --locked
cd ..
TRIAGEPAY_WALLET_WASM=contracts/target/wasm32-unknown-unknown/release/policy_wallet.wasm node demo/live/setup.mjs
```

`setup.mjs` calls `set_review_policy` with the threshold equal to the small
demo payment and `review_unknown_payees` true. Amounts above that payment,
and any unknown payee, wait for the owner. Copy the new account ids from
the printed `config.json` if they changed, and set
`TRIAGEPAY_AGENT_SECRET_KEY` and `TRIAGEPAY_OWNER_SECRET_KEY` in Vercel from
`demo/live/.keys/` (the agent file is the pay-only key). Do not put the
secrets in git or on a command line that gets logged.

Until that deploy, the demo page shows the queue as a labelled walkthrough
and refuses to sign a "queue a new payee" attempt, because the live wallet
would refuse it with `ERR_PAYEE_NOT_ALLOWED` and still spend gas.
