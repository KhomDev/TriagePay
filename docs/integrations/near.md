# NEAR Integration

Research record and operating contract for the NEAR payment adapter
(crates/near-adapter, package aps-near). Every fact below was verified
against the live network or the official documentation on **2026-10-20**;
sources are cited per section. Nothing in this file is aspirational - what
is not implemented is listed as not implemented.

## Scope: verification only

The adapter implements the PaymentProvider semantics with on-chain
**verification** and nothing else:

- It never signs or broadcasts transactions. The payer's wallet executes
  the deposit transfer; the gateway only reads and re-verifies it.
- It never executes refunds on chain. A refund request returns a
  **proposal** (an exact on-chain transfer plan) that requires a
  human-approved ft_transfer from the gateway account.
- Read-only RPC access (status, view_account, view_access_key,
  ft_balance_of, ft_metadata, tx status) is unrestricted and safe.

## Verified facts (2026-10-20)

| Fact | Value | Source |
|---|---|---|
| JSON-RPC protocol | POST to endpoint root, JSON-RPC 2.0, Content-Type application/json required (415 otherwise) | https://docs.near.org/api/rpc/introduction |
| Public mainnet RPC | https://rpc.mainnet.near.org - verified live: chain_id mainnet, height about 220 822 921 | same |
| Public testnet RPC | https://rpc.testnet.near.org | https://docs.near.org/api/rpc/providers |
| Transaction status | method "tx", params tx_hash + sender_account_id + wait_until; wait_until "FINAL" returns only when the tx block and all receipts are finalized; EXPERIMENTAL_tx_status is a deprecated alias of tx_status | https://docs.near.org/api/rpc/transactions |
| NEP-141 transfer | ft_transfer(receiver_id, amount, memo?) as a function call on the token contract; caller must attach **exactly 1 yoctoNEAR**; args are JSON, base64-encoded in the action | https://docs.near.org/primitives/ft/standard |
| Settlement asset on mainnet | a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near - "Bridged USDC", symbol **USDC.e**, **6 decimals**, total supply about 14.6M (verified via ft_metadata and ft_total_supply at finality) | on-chain read, 2026-10-20 |
| Testnet faucet | POST https://faucet-near-testnet.onrender.com/api/faucet with {"accountId": "..."} (accepts named or implicit accounts) | https://docs.near.org/getting-started/faucet |
| Access keys | view_access_key returns permission: the string "FullAccess", or an object naming the contract, method, and allowance of a function-call key | https://docs.near.org/protocol/accounts-contracts/access-keys |

**Privacy RPC caveat (verified 2026-10-20):** a 1RPC endpoint configured as
1rpc.io/&lt;key&gt;/near answered eth_chainId and eth_blockNumber (height
matching NEAR mainnet) but returned JSON-RPC error -32601 ("method does not
exist") for the NEAR methods status, query, and block. It is not usable
for this adapter as configured. The official public endpoints above are
the tested default; any configured NEAR_RPC_URL must answer the NEAR
JSON-RPC surface.

**There is no native Circle USDC on NEAR mainnet** in the verified data: the
canonical USDC-type asset is bridged USDC.e (6 decimals). The price book's
"1 USDC" therefore means 1 000 000 atomic units of the configured asset
contract; the operator's deposited asset must be confirmed with
ft_balance_of once the account id is configured.

## How a deposit funds credit

1. **Create intent** - POST /payment/intents with the account, asset (must
   equal the configured settlement contract), amount, and expiry. The
   response carries deposit_address (the gateway NEAR account), memo
   (equal to the intent id), and provider "near".
2. **Pay** - from their own wallet, the caller executes
   ft_transfer with receiver_id set to the gateway account, amount set to
   the intent amount, and memo set to the intent id, on the asset
   contract, attaching 1 yoctoNEAR. Intent ids are generated
   collision-free across restarts (process tag + synced clock + counter),
   so one memo can only ever name one intent.
3. **Observe** - POST /payment/intents/{id}/observations with the
   transaction hint (signer_id and tx_hash). The hint only locates the
   transaction; the adapter then re-verifies, on chain:
   - the transaction executed successfully and is final (wait_until
     FINAL);
   - it targets the configured asset contract;
   - it contains an ft_transfer action whose arguments name the gateway
     account and carry exactly this intent's memo;
   - it attaches exactly 1 yoctoNEAR;
   - the moved amount (the chain's number, not the client's).
   Structural mismatches (wrong contract, recipient, memo, or deposit) are
   client errors; the intent stays unfunded. An amount mismatch surfaces
   as a **confirmed mismatch** and never funds. A failed transaction
   rejects the intent. A not-yet-visible transaction stays pending.
4. **Fund** - a fully matching observation flows through the unchanged
   pure reconcile function and the idempotent ledger: credit is deposited
   once, keyed by the intent id. Re-observing replays the receipt and
   moves nothing. A client assertion of payment has no route anywhere in
   this path.

## Refunds

POST /payment/intents/{id}/refunds on the near provider returns a proposal
receipt (refund_id "near-proposal-...", reason text explicitly marked
**PROPOSAL ONLY**) describing the exact ft_transfer an operator would
execute from the gateway account. It is one-shot per intent. On-chain
refund execution is deliberately not implemented; see ADR-0005.

## Configuration

| Variable | Meaning | Default |
|---|---|---|
| APS_PROVIDER | mock or near | mock |
| NEAR_NETWORK | mainnet or testnet (required for near) | - |
| NEAR_GATEWAY_ACCOUNT | gateway NEAR account receiving deposits (required) | - |
| NEAR_ASSET_CONTRACT | NEP-141 settlement asset contract (required) | - |
| NEAR_RPC_URL | JSON-RPC endpoint | official public endpoint for the network |
| NEAR_RPC_TIMEOUT_SECS | per-call timeout | 15 |

Keys are never environment variables of this service: the adapter holds no
private keys by design. A .env.example in the repository root documents the
full variable set with placeholders.

## Security-track angle: function-call keys

NEAR access keys can be scoped to a single contract and method (the
permission object's receiver_id and method_names). The recommended
hardening for a real deployment is a **function-call key on the gateway's
funding account restricted to ft_transfer on the asset contract**: the hot
process that moves deposits can then only ever move the settlement asset
in the standard transfer shape, never arbitrary contracts. This is the
NEAR-native answer to "give an agent a payment key, not the vault key" and
maps directly to the hackathon security track. Not yet exercised in the
demo; listed as the first hardening milestone.

## v2 roadmap: NEAR Intents (1Click API)

The cross-chain deposit flow is documented for the next milestone (the
README names Intents as the adapter target):

- Docs: https://docs.near-intents.org/ (1Click API under
  integration/distribution-channels/1click-api).
- Flow: request a quote (EXACT_INPUT, origin/destination asset, amount),
  receive a deposit address (and optional memo), the payer deposits on
  the origin chain, submit a signed intent (NEP-413), then poll
  GET /v0/status with the deposit address.
- Auth: JWT bearer token (account-based) plus an X-API-Key variant.
- Official SDKs: TypeScript @defuse-protocol/one-click-sdk-typescript,
  Rust one_click_sdk_rs, Go openapiclient.
- Why it is v2: the 1Click flow funnels deposits through the Intents
  relayer infrastructure and requires an account-derived JWT; the direct
  NEP-141 flow (v1, implemented) needs no third party and works with the
  operator's wallet today. The provider trait is the seam: an Intents
  adapter slots in behind the same observe semantics.

## Not implemented (explicit)

- Transaction signing and broadcasting of any kind.
- On-chain refund execution (proposals only).
- Indexer/listener-based deposit observation (the caller supplies the tx
  hash hint; a listener would remove that step).
- NEAR Intents 1Click adapter (v2, documented above).
- Function-call-key hardening of a dedicated gateway account.

## Operator-gated next steps (from the session plan)

1. Add NEAR_ACCOUNT_ID (the named account holding the deposited USDC) to
   the .env kept outside this repository. The account id is required for
   the preflight script and the mainnet E2E.
2. Run scripts/near-preflight.py (read-only): confirms the account, its
   access key type, the NEAR balance for gas, and the USDC balance with
   metadata.
3. Execute the mainnet E2E (scripts/near-demo) with an explicit approval
   of the single on-chain transfer it requests.


---

## Operator wallet state (verified read-only, 2026-10-20)

Account **hlpok1lad.tg** on mainnet:

| Fact | Value |
|---|---|
| Account exists | yes (created via a delegate-action flow from the tg/HOT wallet) |
| Access key | the configured ed25519 keypair is registered with **FullAccess** (can sign transfers) |
| Liquid NEAR | **0** (below the ~0.05 gas floor) |
| Tokens held | 0.01 HOT (game.hot.tg); **0 USDC on NEAR** |
| intents.near balance | 0 (the one intents.near tx in history left no parked balance) |
| The 1 USDC | lives on **Base** (per the operator), not on NEAR yet |

Consequences for the demo flow:

1. The account needs a small amount of NEAR for gas before the wallet can
   send the deposit ft_transfer (most wallets can attach gas paid from
   the deposit intent's transfer; if the USDC arrives via a swap that
   delivers NEAR-side tokens, a ~0.05 NEAR top-up is still recommended).
2. The settlement asset contract is set to USDC.e; once the swap lands,
   run scripts/near-wait-for-deposit.py to detect the actual contract the
   funds arrive in, then set NEAR_ASSET_CONTRACT to match.

## Operator guide: moving the 1 USDC from Base to NEAR (your step)

The agent does not hold (and must never hold) the Base-side key, so this
step belongs to the operator. Two supported routes:

**Route A - NEAR Intents (recommended, and it exercises our v2 target):**
1. Open the NEAR Intents flow (https://intents.near.org or a 1Click-integrated
   frontend) and sign in with **hlpok1lad.tg** (the HOT/.tg wallet can
   authenticate it).
2. Request a quote: swap Base USDC -> NEAR-side USDC, exact input 1 USDC.
   The UI returns a deposit address on Base (with a memo) plus the quote.
3. Send the 1 USDC from the Base wallet to that address with the memo.
4. When the intent settles, the USDC (USDC.e or the current canonical
   USDC token on NEAR) lands under the intents account; withdraw to
   hlpok1lad.tg if the UI holds it in intents (the mt_batch_balance_of
   view on intents.near shows parked balances).
5. Run: python scripts/near-wait-for-deposit.py --account hlpok1lad.tg
   It prints the contract id to set as NEAR_ASSET_CONTRACT.

**Route B - an exchange that supports NEAR withdrawals:** withdraw USDC
on the NEAR network directly to hlpok1lad.tg, then run the same waiter
script. Verify the token contract it reports before setting
NEAR_ASSET_CONTRACT.

After either route: python scripts/near-preflight.py must print READY,
then scripts/near-demo.ps1 walks the verified end-to-end deposit.


---

## Live testnet end-to-end run (2026-10-20, evidence recorded)

Executed against NEAR testnet with throwaway accounts (zero value; keys
generated for the run, held outside the repository):

| Step | Result |
|---|---|
| Payer account | aps-e2e-mvh0s6bh.testnet (helper-created, 10 NEAR) |
| Gateway account | aps-gw-mvh1jptk.testnet (helper-created, 10 NEAR) |
| Settlement asset | wrap.testnet (wNEAR, 24 decimals) - storage registered for both |
| Wrap 1 NEAR to wNEAR | tx 6v7cbphrSsqPDc4o3idfed7zyKErRTZ4NX6Xx7DYiU25, success |
| Intent | aps2_3b74_6ad7b8c5_00001, deposit to gateway, memo-bound |
| Payment (ft_transfer, 1 yoctoNEAR, memo) | tx kNZVYwd9zbf9ugkSvQsTQvwnAGuXbcoAExiXUWfz9xs, success |
| Observation through the adapter | reconciled: fund, funded: true, credited exactly once |
| Re-observation (replay attack) | receipt replayed, deposited_total unchanged |
| Reserve -> scan -> settle | job_0000000000000001, decision supported / confidence high |
| Scan replay after settlement | 409 illegal_transition (terminal; no double charge) |
| Refund request | near-proposal-00001, PROPOSAL ONLY, one-shot (second is 409) |
| Conservation | deposited == settled_total, available 0, reserved 0 |

The payer-side transfer used the official near-api-js (reference
signing); the gateway side is this repository's adapter, verifying the
transaction on chain exactly as in production.

**Topology note discovered by the run:** NEP-141 contracts reject
self-transfers (sender == receiver panics in the reference
implementation). A deployment where the payer pays the intent from the
gateway's own account therefore cannot work by design; the demo
presupposes distinct payer and gateway accounts, which is also the honest
production topology. Record this in any operator runbook.

**Borsh notes for implementers (hard-won):** the Transaction borsh field
order is signer_id, public_key, nonce, receiver_id, block_hash, actions
(receiver BEFORE block_hash - the reverse of the RPC JSON field order),
and the Action enum variant order is CreateAccount, DeployContract,
FunctionCall(2), Transfer, Stake, AddKey, DeleteKey, DeleteAccount,
Delegate. Hand-rolled encoders that get either wrong produce either a
parse error or an InvalidSignature from the node.
