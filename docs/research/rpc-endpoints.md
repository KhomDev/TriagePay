# NEAR RPC endpoints and transaction-wait options (research, 2026-10-21)

Scope: prompt 09 ("re-check the endpoint guidance") and the RPC facts that prompts 02, 03 and 08
depend on. Status legend as in `near-contract-toolchain.md`. All access dates are 2026-10-21.

## 1. Official public endpoints

| Claim | Source | Accessed | Status |
|---|---|---|---|
| `https://rpc.mainnet.near.org` answered `status` (chain `mainnet`, protocol 86, nearcore 2.13.4, height about 220 977 659); three consecutive requests returned HTTP 200 with no rate-limit headers (Cloudflare-fronted). | live read-only calls | 2026-10-21 | Verified-primary |
| `https://rpc.testnet.near.org` answered `status` (chain `testnet`, protocol 88, nearcore 2.14.0-rc.5). | live read-only call | 2026-10-21 | Verified-primary |
| The docs provider page lists NEAR's own entries as `archival-rpc.mainnet.near.org` and `archival-rpc.testnet.near.org` with the note "Severely rate limited"; it shows **no deprecation notice** for `rpc.mainnet.near.org` / `rpc.testnet.near.org` (its curl example still uses `rpc.mainnet.near.org`). | https://docs.near.org/api/rpc/providers | 2026-10-21 | Verified-primary (page content); actual quotas Unverified |
| Third-party endpoints listed (mainnet): FASTNEAR `https://free.rpc.fastnear.com`, 1RPC `https://1rpc.io/near`, Ankr `https://rpc.ankr.com/near`, BlockPI, dRPC `https://near.drpc.org`, GetBlock (key), Intear `https://rpc.intea.rs`, NodeReal (key), NOWNodes (key), Shitzu, ZAN. Testnet: FASTNEAR `https://test.rpc.fastnear.com`, dRPC, Intear, ZAN. | same | 2026-10-21 | Verified-primary |
| FASTNEAR free mainnet answered `status` (mainnet, protocol 86, 2.13.4). | live read-only call | 2026-10-21 | Verified-primary |
| A keyed 1RPC NEAR endpoint did not serve NEAR JSON-RPC methods in the 2026-10-20 test; see `docs/integrations/near.md`. The keyless `https://1rpc.io/near` listed in the docs was not tested. | near.md; providers page | 2026-10-21 | Verified-primary (prior test); keyless endpoint Unverified |

## 2. `wait_until` options

From https://docs.near.org/api/rpc/transactions (accessed 2026-10-21), Verified-primary:

| Value | Returns when |
|---|---|
| `NONE` | queued, not yet in a block |
| `INCLUDED` | included in a block (may not be final) |
| `EXECUTED_OPTIMISTIC` (default) | included and all non-refund receipts executed (blocks may not be final) |
| `INCLUDED_FINAL` | included in a finalized block |
| `EXECUTED` | finalized and all non-refund receipts executed |
| `FINAL` | finalized and all receipts, including refunds, finalized |

- "Higher `wait_until` levels can take several seconds"; on `TIMEOUT_ERROR` the docs advise to
  "Lower `wait_until` and poll `tx`". Newer nodes add an optional `cause` to the timeout.
- `final_execution_status` in the response echoes the level reached (`TxExecutionStatus`).

## 3. Method-name drift (important)

| Claim | Source | Accessed | Status |
|---|---|---|---|
| The docs say `EXPERIMENTAL_tx_status` "is a deprecated alias of `tx_status`" with identical request and response types. | docs transactions page | 2026-10-21 | Verified-primary |
| This rename is in nearcore's unreleased/2.14 line (#16270). **Mainnet 2.13.4 returns `METHOD_NOT_FOUND` for `tx_status`**; `EXPERIMENTAL_tx_status` works on both networks; testnet 2.14 accepts both. | nearcore CHANGELOG; live calls | 2026-10-21 | Verified-primary |
| `tx` omits the `receipts` array; `EXPERIMENTAL_tx_status` includes it (receipt bodies with `predecessor_id` and actions). | live calls | 2026-10-21 | Verified-primary |
| nearcore 2.14 also changes `view_access_key` / `view_access_key_list` errors for missing accounts (`UNKNOWN_ACCOUNT`) and adds pagination to the key list (see `nep413-signed-messages.md`). Clients must accept both the 2.13 and the 2.14 shapes while mainnet upgrades. | nearcore CHANGELOG 2.14.0 | 2026-10-21 | Verified-primary |

## 4. Read-only calls used by this research

All calls were HTTP POSTs of JSON-RPC 2.0, finality `final` where applicable, with no keys:
`status`, `gas_price`, `EXPERIMENTAL_protocol_config`, `tx`, `EXPERIMENTAL_tx_status`,
`tx_status`, `query` (`view_account`, `view_access_key_list`, `call_function` for
`storage_balance_bounds` and `ft_metadata`). No transaction was signed or sent.

## Implications for the build

- **Prompt 03 / 09:** use `EXPERIMENTAL_tx_status` with `wait_until: "FINAL"` (accepted on both
  networks today). Treat `tx_status` as a future name; a fallback order of `tx_status` then
  `EXPERIMENTAL_tx_status` on `METHOD_NOT_FOUND` survives the mainnet upgrade.
- **Prompt 09:** keep `https://rpc.mainnet.near.org` as the default (live, no deprecation notice
  found). Configure FASTNEAR free (`https://free.rpc.fastnear.com`) as a documented fallback for
  the nightly read-only workflow. Re-check the providers page at campaign start, because quotas
  are not published.
- **Prompt 09:** `verify-evidence.py` balance checks at historical blocks need an archival
  endpoint (`archival-rpc.mainnet.near.org`, "Severely rate limited"); otherwise check outcome
  facts only, as the prompt allows.
- **Prompt 03 / 08:** handle a `TIMEOUT_ERROR` as "not final yet" (retry), never as failure.
