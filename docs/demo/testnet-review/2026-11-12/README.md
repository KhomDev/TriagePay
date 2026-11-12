# Testnet run: owner review wallet, 2026-11-12

I deployed a fresh policy wallet with the owner-review queue on NEAR
testnet and ran every path the page shows against it. The hosted demo
signs for these accounts now. Raw record: [`EVIDENCE.json`](EVIDENCE.json).
Scripts: `demo/live/setup.mjs` (deploy) and `demo/live/proof.mjs` (proof,
hosted keys, key cleanup).

## Accounts

| Role | Account |
|---|---|
| Wallet (0 access keys) | [`tpd-e947e7-wallet.testnet`](https://testnet.nearblocks.io/address/tpd-e947e7-wallet.testnet) |
| Agent (one key: `pay` on the wallet) | [`tpd-e947e7-agent.testnet`](https://testnet.nearblocks.io/address/tpd-e947e7-agent.testnet) |
| Owner | [`tpd-e947e7-owner.testnet`](https://testnet.nearblocks.io/address/tpd-e947e7-owner.testnet) |
| Allowlisted payee | [`tpd-e947e7-payee.testnet`](https://testnet.nearblocks.io/address/tpd-e947e7-payee.testnet) |
| Flagged payee | [`tpd-e947e7-flagged.testnet`](https://testnet.nearblocks.io/address/tpd-e947e7-flagged.testnet) |
| Injected payee (never created) | `tpd-e947e7-injected.testnet` |

Token `wrap.testnet`. Per-call cap 0.001 wNEAR, daily cap 0.02 wNEAR, demo
payment 0.0001 wNEAR. Review policy: `approval_threshold` = 0.0001 wNEAR
(an amount strictly above it waits for the owner), `review_unknown_payees`
= false (a payee outside the allowlist is still `ERR_PAYEE_NOT_ALLOWED`, so
the injection demo stays a refusal). Wasm: 237,665 bytes, SHA-256
`52e47935c7649ceee9ec43e07cff9ff2c844e6271c7730d0a452459820d4c2c6`, built
on Windows with `cargo build --release` and `--remap-path-prefix` so it
carries no host paths. Funding came from the public testnet faucet (5 NEAR).

## Setup

| Step | Transaction |
|---|---|
| Create owner, agent, payee, flagged, wallet | [owner](https://testnet.nearblocks.io/txns/9ZQWjMR1UBF1U7dADzgzCt7PWo1GXQ8T98EYcWuiZ7kZ), [agent](https://testnet.nearblocks.io/txns/DjK1c4XAUHgfD2uwGVf62Bf2xa7aqL3inwz5GT9pdNDK), [payee](https://testnet.nearblocks.io/txns/CicB2zzizhYRFwUrZWxs64SpUDbp873mQLyqDKhyYMUH), [flagged](https://testnet.nearblocks.io/txns/ApbkqpNyorB8aDLTqRXPnTKMBnHBxowR9r7fYRXBeDK5), [wallet](https://testnet.nearblocks.io/txns/2krtunHP679qvSKF7JM9CuocXMopU3BJNv8gpaGrYPQv) |
| Deploy, `new`, delete the wallet's only key (one batch) | [FrFqgJwU](https://testnet.nearblocks.io/txns/FrFqgJwUsjC8Xq21fsKhvyda8PumnXcN7wtQW2gx2xLd) |
| Wrap 0.05 NEAR, register wallet and payee, fund the wallet | [Can5gowr](https://testnet.nearblocks.io/txns/Can5gowr7euHRj1mWKE9zSQm36eJUqjS4ud58pHSMaSu), [Dmc7J2Sf](https://testnet.nearblocks.io/txns/Dmc7J2SfZnbr6giLJZj7AqcEQqd6gQBpnLNmyZMfUfVk), [FfZWC1Qv](https://testnet.nearblocks.io/txns/FfZWC1QvWX2TbmZirXVsPNE5frXHjAvKQYsAtU5thV6c) |
| `set_agent` | [Co3E88Wt](https://testnet.nearblocks.io/txns/Co3E88WtftzL8j2E7qjVF2ndfVf7SkCFaJndonFaJwmV) |
| `set_review_policy(threshold, false)` | [AQTbvpnP](https://testnet.nearblocks.io/txns/AQTbvpnPFdhxwWK8hHKK5Xg6PRq3R81wrJL8g7LLrwtt) |
| Add a local pay key for the proof | [7crwX8qt](https://testnet.nearblocks.io/txns/7crwX8qt4c3YMKd4hfGYknJHEbZkvoBQbkjgn3FTWAmp) |

## Proof

| # | What | Result | Transaction |
|---|---|---|---|
| 1 | Allowed payment, 0.0001 wNEAR to the allowlisted payee | settled, payee balance +0.0001 | [4oxycWdv](https://testnet.nearblocks.io/txns/4oxycWdvo9M4YWoq9mg2y4FXL7AZYQY6McvcPJQYJejq) |
| 2 | Prompt-injected payee | `ERR_PAYEE_NOT_ALLOWED`, nothing moved | [8y6sEwL8](https://testnet.nearblocks.io/txns/8y6sEwL8i5sP2aMKo5HS3WGZtZ8NMDP9MUdywYAmVnN4) |
| 3 | Over the per-call cap by one atomic unit | `ERR_PER_CALL_LIMIT` | [BMoHjQXg](https://testnet.nearblocks.io/txns/BMoHjQXgQasLxn4wZspzTQU9g1rBjbqThhZQevknCZ7s) |
| 4 | 0.0002 wNEAR, above the threshold | queued as payment 1 (`above_threshold`), payee balance unchanged | [8nvs4X4j](https://testnet.nearblocks.io/txns/8nvs4X4jWrzEzsbUj2ziS5qEdTTi6wvRwfUTBxftdcJi) |
| 5 | Owner `approve_payment(1)` | settled, payee balance +0.0002 | [CwEo7ec8](https://testnet.nearblocks.io/txns/CwEo7ec8uk25XQbKHNmS3naUhqM9FPbRJzEAjVf223Hp) |
| 6 | 0.0003 wNEAR, above the threshold | queued as payment 2 | [J9ZzGEQP](https://testnet.nearblocks.io/txns/J9ZzGEQP1i8H1pGq9VMhy8L2nWjwRy5qDVWQqW5wDk5g) |
| 7 | Owner `reject_payment(2)` | dropped, reservation returned, payee balance unchanged | [CcUmBvhQ](https://testnet.nearblocks.io/txns/CcUmBvhQPBvtPJ9pg4NF225U4XfaJEmwjLEZV9gCAaWR) |
| 8 | Owner `revoke_agent` | agent inactive | [5qZ5yQAP](https://testnet.nearblocks.io/txns/5qZ5yQAPTm5dFg7i455HvsrgBNk8bTbbCou6zq4461as) |
| 9 | Revoked agent pays | `ERR_AGENT_INACTIVE` | [3iHytU9n](https://testnet.nearblocks.io/txns/3iHytU9nkYuqPh74its9TPW3MwQoUJeaPQg1nbGGyQTZ) |
| 10 | Owner `set_agent` again, so the hosted page starts active | active, spend for the day kept (0.0003) | [8GihEqyX](https://testnet.nearblocks.io/txns/8GihEqyXbePiv4s1yKbvmr6YVzKCBAz4PbVaw4G4yCgH) |

After step 10 the wallet held 0.0497 wNEAR, the payee 0.0003 wNEAR, and
`get_pending` was empty.

One mistake: my first `approve_payment` call passed an undefined id
([BtDTa1Pw](https://testnet.nearblocks.io/txns/BtDTa1PwAEpuEHNV5KSLuynVUHkNPWEHYXJ26HQBa4a2)).
The wallet refused to parse the arguments and changed nothing. I fixed the
field name (`payment_id`) and continued from step 5.

## Hosted keys and key cleanup

The Vercel project already held two secrets: the pay key and the owner key
of the previous demo accounts. Instead of rotating them, I added their
public halves to the new accounts. No secret left the Vercel project.

| Step | Transaction |
|---|---|
| Agent: add `ed25519:9iERawak26p4uGVfktNgcpG1zxuEV31d8GAnY1qUxUis` as a function-call key, `pay` on `tpd-e947e7-wallet.testnet` only, 0.25 NEAR gas allowance | [2sfo5j9N](https://testnet.nearblocks.io/txns/2sfo5j9NdZGT3Bb3ap9FpfXaPjtzfCZfkEB5iQCJXByZ) |
| Owner: add `ed25519:9GNK747csiMRCG8iTyLk6Jb75SKDBVFExwfkn49UMNua` as a full-access key | [D9Vghe56](https://testnet.nearblocks.io/txns/D9Vghe56X4yY47awicJoo4Gaj3KPN3xmdHLmT1qfMZZp) |
| Agent: delete the local pay key and the setup full-access key, in one transaction | [74a2PEiV](https://testnet.nearblocks.io/txns/74a2PEiV98G5gJppaiXTNZ2vmGBCEDbPxUAuSzPpsAsC) |

After cleanup, `view_access_key_list` on the agent returns one key: the
hosted pay key. The agent account has no full-access key, so when that
key's gas allowance runs out it cannot be topped up and a new agent account
is needed. The owner account keeps two full-access keys: the one setup
created (on my machine) and the hosted one.

## Hosted preview, same accounts

Before the cleanup I checked that the Vercel secrets sign for the new
accounts, from the preview deployment of this branch:

| What | Result | Transaction |
|---|---|---|
| `/api/run` injected payee (hosted agent key) | `ERR_PAYEE_NOT_ALLOWED` | [BMBXbcer](https://testnet.nearblocks.io/txns/BMBXbcerQFiEyuM1swpuSYf25kJuWD5MjhCXWqobQnB5) |
| `/api/run` above threshold | queued as payment 3 | [HpPF9LQd](https://testnet.nearblocks.io/txns/HpPF9LQd1vz4UN5EtqVN5edAE4cTYzb8gdjqV6YJpXxK) |
| `/api/review` approve 3 (hosted owner key) | settled | [81buZdY4](https://testnet.nearblocks.io/txns/81buZdY4SqNEEZrR7Z17p1K9LQNNwdYj6X8G2oCWeyRi) |
