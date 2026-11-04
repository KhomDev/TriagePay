# NEP-141 events and finalized receipts (research, 2026-10-21)

Scope: prompt 03 research items 3-4. This covers the exact NEP-141 `ft_transfer` event, the
`tx` / `EXPERIMENTAL_tx_status` response shape at `wait_until: FINAL`, the status enums, a real
mainnet example, and the spoofing argument for a receipt-level verifier.

Status legend as in `near-contract-toolchain.md`. All access dates are 2026-10-21.

## 1. The NEP-141 transfer event (NEP-297)

| Claim | Source | Accessed | Status |
|---|---|---|---|
| FT events "MUST have `standard` set to `"nep141"`, standard version set to `"1.0.0"`", and `event` is one of `ft_mint`, `ft_burn`, `ft_transfer`. | https://github.com/near/NEPs/blob/master/neps/nep-0141.md (Events section) | 2026-10-21 | Verified-primary |
| `data` is an **array** (`FtTransferLog[]`); one log line can carry several transfers. Each `FtTransferLog` has `old_owner_id: string`, `new_owner_id: string`, `amount: string` (u128 as decimal string), and optional `memo?: string`. | same | 2026-10-21 | Verified-primary |
| The line format is `EVENT_JSON:` followed by one JSON object; whitespace inside is allowed. | https://github.com/near/NEPs/blob/master/neps/nep-0297.md | 2026-10-21 | Verified-primary |
| In the reference implementation, `ft_resolve_transfer` (the callback of `ft_transfer_call`) emits a second `ft_transfer` event in the **reverse** direction with `memo: "refund"` when the receiver returns unused tokens, or an `ft_burn` with `memo: "refund"` if the sender account was deleted. | `near-contract-standards/src/fungible_token/core_impl.rs` at tag `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary (reference implementation); behaviour of the native USDC contract's own code is Unverified |

## 2. Transaction status RPC

| Claim | Source | Accessed | Status |
|---|---|---|---|
| `wait_until` values: `NONE`, `INCLUDED`, `EXECUTED_OPTIMISTIC` (default), `INCLUDED_FINAL`, `EXECUTED` ("Finalized + all non-refund receipts executed"), `FINAL` ("Finalized + all receipts (including refunds) finalized"). | https://docs.near.org/api/rpc/transactions | 2026-10-21 | Verified-primary |
| `tx` returns `final_execution_status`, `status`, `transaction`, `transaction_outcome`, `receipts_outcome`. `EXPERIMENTAL_tx_status` returns the same plus `receipts` (the receipt bodies with `predecessor_id`, `receiver_id`, actions). | live read-only calls on mainnet (see section 4) | 2026-10-21 | Verified-primary |
| nearcore (unreleased on mainnet) renames `EXPERIMENTAL_tx_status` to `tx_status` and keeps the old name as a deprecated alias. The docs already describe this. **Mainnet 2.13.4 answers `tx_status` with `METHOD_NOT_FOUND`**; testnet 2.14.0-rc.5 answers both names. | nearcore CHANGELOG (#16270); live calls on both networks | 2026-10-21 | Verified-primary |
| Each `receipts_outcome[]` element is `{id, block_hash, outcome: {executor_id, gas_burnt, tokens_burnt, logs[], receipt_ids[], status, metadata}, proof}`. `receipt_ids` are the child receipts, so the tree is walked from `transaction_outcome.outcome.receipt_ids`. | live response | 2026-10-21 | Verified-primary |
| Timeouts return `TIMEOUT_ERROR`; newer nodes add an optional `cause` (`NOT_OBSERVED`, `PENDING`, `DOES_NOT_TRACK_SHARD`, `ERROR`). | docs page above; nearcore CHANGELOG | 2026-10-21 | Verified-primary |

### Status enums (nearcore tag 2.13.4, `core/primitives/src/views.rs`)

| Level | Enum | Variants |
|---|---|---|
| Top-level `status` of the transaction | `FinalExecutionStatus` | `NotStarted`, `Started`, `Failure(TxExecutionError)`, `SuccessValue(base64)` |
| `transaction_outcome.outcome.status` and every `receipts_outcome[].outcome.status` | `ExecutionStatusView` | `Unknown`, `Failure(TxExecutionError)`, `SuccessValue(base64)`, `SuccessReceiptId(CryptoHash)` |
| `final_execution_status` | `TxExecutionStatus` | `NONE`, `INCLUDED`, `EXECUTED_OPTIMISTIC`, `INCLUDED_FINAL`, `EXECUTED`, `FINAL` |

**Answer to the prompt 03 question:** `SuccessReceiptId` is **not** a variant of
`FinalExecutionStatus`, so it can never appear in the top-level `status`. It is normal at the
outcome level: the transaction outcome of every function-call transaction is
`SuccessReceiptId(<first receipt>)`, as the example below shows. A parser that accepts
`SuccessReceiptId` as top-level success is accepting a shape the node never sends; it should
treat it as a structural error. At `wait_until: FINAL` only `SuccessValue` and `Failure` are
expected at the top level (`NotStarted` / `Started` belong to weaker wait levels).

Source: https://raw.githubusercontent.com/near/nearcore/2.13.4/core/primitives/src/views.rs
(accessed 2026-10-21), Verified-primary.

## 3. What top-level success does and does not prove

- The top-level `SuccessValue` is the result of the transaction's main promise chain. A contract
  whose callback handles a failed downstream call and returns normally (exactly what the prompt
  02 `on_pay_complete` does when it releases a reservation) produces a top-level
  `SuccessValue` even though the `ft_transfer` receipt **failed**. Top-level success is therefore
  necessary but not sufficient; the token receipt's own status must be checked. (Derived from the
  enum semantics and the cross-contract docs: "Your state will not rollback automatically".)
- A receipt's `logs` may contain lines written before a later failure in the same receipt, so a
  log line is only meaningful when that receipt's own status is a success variant. Status:
  Likely; verify with the prompt 03 "failed transfer" fixture (a receipt that logs and then
  panics) on testnet.
- `ft_transfer_call` to the gateway is a trap: the outgoing `ft_transfer` event (memo = intent id)
  appears in a successful token receipt, then `ft_on_transfer` on the gateway fails (a plain
  account has no contract) and `ft_resolve_transfer` refunds the amount with a reverse
  `ft_transfer` event. A verifier that only looks for "one event to the gateway with the memo"
  would credit a payment that was refunded. Defences: (a) require the token receipt's
  `FunctionCall.method_name == "ft_transfer"` (needs the `receipts` array, i.e.
  `EXPERIMENTAL_tx_status`), and/or (b) compute the **net** gateway delta over all `nep141`
  events emitted by the asset executor in the transaction, subtracting events whose
  `old_owner_id` is the gateway. Status: Verified-primary for the reference-implementation
  behaviour; recommendation for the defence.

## 4. Real mainnet example (read-only)

Call: `EXPERIMENTAL_tx_status` on https://rpc.mainnet.near.org with
`{"tx_hash":"ZonAdEjgAUM4tBnCR3nJTxe9LHiv7KcsbqRZdp9dns1","sender_account_id":"hlpok1lad.tg","wait_until":"FINAL"}`,
accessed 2026-10-21. Abridged:

```json
{
  "final_execution_status": "FINAL",
  "status": {"SuccessValue": ""},
  "transaction": {
    "signer_id": "hlpok1lad.tg",
    "receiver_id": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
    "actions": [{"FunctionCall": {"method_name": "ft_transfer", "deposit": "1",
                                   "gas": 30000000000000, "args": "<base64 JSON>"}}]
  },
  "transaction_outcome": {
    "id": "ZonAdEjgAUM4tBnCR3nJTxe9LHiv7KcsbqRZdp9dns1",
    "outcome": {"executor_id": "hlpok1lad.tg",
                "status": {"SuccessReceiptId": "HcfgQShNoZY1czCKBLdK9TzUxXPCV5rcp93kB6efWPf2"},
                "receipt_ids": ["HcfgQShNoZY1czCKBLdK9TzUxXPCV5rcp93kB6efWPf2"]}
  },
  "receipts_outcome": [
    {"id": "HcfgQShNoZY1czCKBLdK9TzUxXPCV5rcp93kB6efWPf2",
     "outcome": {"executor_id": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
                 "status": {"SuccessValue": ""},
                 "gas_burnt": 2320749078992,
                 "logs": ["EVENT_JSON:{\"standard\":\"nep141\",\"version\":\"1.0.0\",\"event\":\"ft_transfer\",\"data\":[{\"old_owner_id\":\"hlpok1lad.tg\",\"new_owner_id\":\"gateway.hlpok1lad.tg\",\"amount\":\"100000\",\"memo\":\"aps2_2160_6ad8a68a_00001\"}]}"],
                 "receipt_ids": ["4ppvyQb8ad4kE4xAYh9eWYq6P2Vjr9GQ6s14tkizdVRY"]}},
    {"id": "4ppvyQb8ad4kE4xAYh9eWYq6P2Vjr9GQ6s14tkizdVRY",
     "outcome": {"executor_id": "hlpok1lad.tg", "status": {"SuccessValue": ""},
                 "logs": [], "receipt_ids": []}}
  ],
  "receipts": [
    {"receipt_id": "HcfgQShNoZY1czCKBLdK9TzUxXPCV5rcp93kB6efWPf2",
     "predecessor_id": "hlpok1lad.tg",
     "receiver_id": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"},
    {"receipt_id": "4ppvyQb8ad4kE4xAYh9eWYq6P2Vjr9GQ6s14tkizdVRY",
     "predecessor_id": "system", "receiver_id": "hlpok1lad.tg",
     "receipt": {"Action": {"actions": [{"Transfer": {"deposit": "30656210421434800000000"}}]}}}
  ]
}
```

Observations (all Verified-primary from this response):

- Receipt executors: the signer (transaction outcome), then the token contract (the
  `ft_transfer` receipt, which carries the `EVENT_JSON` line), then the signer again (a gas-refund
  receipt from `system`).
- The refund receipt returned 0.0307 NEAR: gas was prepaid at 0.001 NEAR/Tgas for 30 Tgas and
  mostly refunded. Under `wait_until: FINAL` the response waits for refund receipts too.
- The plain `tx` method returned the same structure without `receipts`.

## 5. Spoofing risk and the executor binding

- Any contract can call `log` with a string that starts with `EVENT_JSON:` and claims
  `"standard":"nep141"`. NEP-297 defines a format, not an authenticity mechanism. A malicious
  contract on the payment path (for example a fake "wallet" between payer and token) can log a
  perfect-looking `ft_transfer` event to the gateway with any memo and amount.
- The only party that can write a log line under `executor_id == <asset contract>` is code
  running as the asset contract account. Binding acceptance to `outcome.executor_id ==
  intent.asset_id` therefore reduces trust to the token contract itself. That contract is
  upgradeable by its key holder (the native USDC account holds one FullAccess key, per a live
  `view_access_key_list`, 2026-10-21), which is an accepted trust assumption of accepting that
  asset at all.
- Remaining checks on the bound receipt: its own status is `SuccessValue`; the event parses;
  `new_owner_id == gateway`; `memo == intent_id`; `amount` parses as u128; the method was
  `ft_transfer` or the net delta accounts for refund events (section 3); at most one funding
  event per intent.
- Wallet-originated payments verify the same way: the `pay` transaction's receipt tree contains
  a receipt executed by the asset contract whose predecessor is the wallet, and the event's
  `old_owner_id` is the wallet account.

## Implications for the build

- **Prompt 03:** call `EXPERIMENTAL_tx_status` (works on mainnet 2.13.4 and testnet 2.14), not
  `tx_status` (mainnet `METHOD_NOT_FOUND`) and not `tx` (no `receipts`, so no method names or
  predecessors). Optionally try `tx_status` first and fall back.
- **Prompt 03:** tighten `parse_status`: top-level accepts only `SuccessValue` (confirmed) and
  `Failure` (failed); `NotStarted` / `Started` map to not-final; `SuccessReceiptId` at the top
  level is a structural error. Keep `SuccessReceiptId` valid at the outcome level.
- **Prompt 03:** walk `receipts_outcome` by id; consider only receipts with `executor_id ==
  asset`, a success status, and logs starting with `EVENT_JSON:`; parse `data` as an array.
- **Prompt 03:** add the `ft_transfer_call` refund case to the fixture list (it is the most
  realistic bypass of a naive event check), alongside the spoofed-event contract.
- **Prompt 03:** do not treat top-level `SuccessValue` of a wallet `pay` as proof of payment; the
  wallet callback can succeed while the transfer failed.
- **Prompt 09:** evidence records should store the token-receipt id, its executor, and a digest
  of its logs, as the example shows.
