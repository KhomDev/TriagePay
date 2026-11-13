# Hosted demo: running and deploying it

The judge page is `demo/live`, deployed at https://triagepay-demo.vercel.app/. What it shows is in the [README](../../README.md#hosted-demo). This page is for running or deploying it.

The browser never sees a key. Revoke, Restore, Approve and Reject are public buttons. They
use the owner account's full-access key, which is held on the server as
`TRIAGEPAY_OWNER_SECRET_KEY`.

An allowlisted payee shows warn when the account is under seven days old.
The demo payee was created for the page, so warn is expected. Warn does
not move money. The risk check reads the allowlist from the wallet
contract, the same list the spend panel shows. If a visitor leaves the
agent revoked, the next Launch restores it before the attack, and a daily
cron calls the same restore.

Required Vercel environment variables (names only; values stay out of git):

| Variable | What it is |
|---|---|
| `TRIAGEPAY_AGENT_SECRET_KEY` | `ed25519:...` function-call key. It can call only `pay` on the wallet. |
| `TRIAGEPAY_OWNER_SECRET_KEY` | `ed25519:...` owner key. Used only to revoke and restore the agent and to approve or reject a held payment. |

Public account ids, the RPC URL, limits, the flagged account, and the
verified code hash live in `demo/live/config.json` after setup. Optional
overrides: `TRIAGEPAY_RPC_URL`, `TRIAGEPAY_OWNER_ACCOUNT`,
`TRIAGEPAY_AGENT_ACCOUNT`, `TRIAGEPAY_WALLET_ACCOUNT`,
`TRIAGEPAY_TOKEN_ACCOUNT`, `TRIAGEPAY_PAYEE_ACCOUNT`,
`TRIAGEPAY_INJECTED_ACCOUNT`, `TRIAGEPAY_FLAGGED_ACCOUNT`,
`TRIAGEPAY_COOLDOWN_SECONDS` (default 20), `TRIAGEPAY_MAX_ATTEMPTS_PER_DAY`
(default 40). The full template is `demo/live/.env.example`.

The server signs at most once per cooldown, at most the daily attempt cap,
and at most a few times a minute from one address
(`TRIAGEPAY_IP_LIMIT`, default 4, per `TRIAGEPAY_IP_WINDOW_SECONDS`,
default 60). Those three hold across serverless instances when a shared
store is configured: `KV_REST_API_URL` and `KV_REST_API_TOKEN`, or
`UPSTASH_REDIS_REST_URL` and `UPSTASH_REDIS_REST_TOKEN`. Create the store
in the Vercel or Upstash dashboard and add the two variables with
`npx vercel env add`. Do not pass the token on the command line. If the
store is set and does not answer, signing is refused. If it is unset, the
daily cap is read from the chain index (paginated through the UTC day, not
the last eight transactions) plus this instance's journal, and the
per-address cap is only inside that instance. A truncated index fails
closed. Refused attempts are still transactions on the explorer. The
hosted project has no store connected yet (`/api/health` reports
`sharedLimits: false`), so it runs on that fallback.

The function-call key also has a limited gas allowance, and the contract's
own daily limit caps the token.

```sh
cd demo/live
npm ci
npx vercel link
npx vercel env add TRIAGEPAY_AGENT_SECRET_KEY production
npx vercel env add TRIAGEPAY_OWNER_SECRET_KEY production
npx vercel deploy --prod
```

The CLI prompts for each secret. Do not pass the key on the command line.
A local run of the same functions:

```sh
cd demo/live
npm ci
node server.mjs
```

When `VERCEL` is unset and the two variables are empty, the server reads
`demo/live/.keys/` (gitignored). On Vercel that fallback is off.

Setup (once, from a machine that can reach the testnet faucet) builds
nothing by itself. Point it at the policy-wallet wasm, then copy the two
secrets into Vercel:

```sh
cd contracts
# --cfg near is what cargo-near adds. Without it, env::panic_str does not
# call the host and a refusal is a silent trap.
RUSTFLAGS='--cfg near' cargo build --release --target wasm32-unknown-unknown -p policy-wallet --locked
cd ..
TRIAGEPAY_WALLET_WASM=contracts/target/wasm32-unknown-unknown/release/policy_wallet.wasm node demo/live/setup.mjs
```

`setup.mjs` prints account ids and writes keys under `demo/live/.keys/`.
It does not print the secrets. The public demo accounts on testnet are in
`demo/live/config.json`: wallet `tpd-e947e7-wallet.testnet` (with the
owner-review queue, deployed 2026-11-12), agent
`tpd-e947e7-agent.testnet`, owner `tpd-e947e7-owner.testnet`.
`node demo/live/proof.mjs` runs the live proof against them; the
transactions are in
[docs/demo/testnet-review/2026-11-12](docs/demo/testnet-review/2026-11-12/README.md).

The agent account has one access key: the function-call key that can call
only `pay` on that wallet
(`ed25519:9iERawak26p4uGVfktNgcpG1zxuEV31d8GAnY1qUxUis`). That is the key
the Vercel project already held for the previous demo wallet. I added its
public half to the new agent
([2sfo5j9N](https://testnet.nearblocks.io/txns/2sfo5j9NdZGT3Bb3ap9FpfXaPjtzfCZfkEB5iQCJXByZ)), and the
hosted owner key to the new owner
([D9Vghe56](https://testnet.nearblocks.io/txns/D9Vghe56X4yY47awicJoo4Gaj3KPN3xmdHLmT1qfMZZp)), so no secret
had to move. Then I deleted the setup full-access key and my local pay key
in one transaction
([74a2PEiV](https://testnet.nearblocks.io/txns/74a2PEiV98G5gJppaiXTNZ2vmGBCEDbPxUAuSzPpsAsC)). With no
full-access key left, the agent cannot add a key or move its own NEAR. When
the pay key's 0.25 NEAR gas allowance runs out, the agent needs a new
account.

`node demo/live/setup.mjs cleanup` does the same deletion for an agent set up the old way (`--drop-full-access` also removes the full-access key, and only when a pay key remains). The previous demo agent, `tpd-797021-agent.testnet`, still holds its old keys because I do not have its full-access secret; the page no longer uses it.

## Production check, 2026-11-13

After the merge of 44e7955 I deployed this directory to production and ran the page once:
Launch attack was [refused with `ERR_PAYEE_NOT_ALLOWED`](https://testnet.nearblocks.io/txns/BxhNWAMyu2sQmA6Ep1qytu5MXDjShRgcNJLG46inVXNE), and a payment above the threshold was [held](https://testnet.nearblocks.io/txns/FLABkn4Tm2T7nbqaGYSTJXMYdLATozJ11hSSHuBda5mT), then [approved and settled](https://testnet.nearblocks.io/txns/Eqz4okPY57FFLWUGTPJDCYc5oNr6ybXCrHh4Sp9TMeyb).
