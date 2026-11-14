# Trail of Bits skill run 2026-11-14: findings and fixes

Nine Trail of Bits audit skills (context building, entry points, sharp
edges, token integration, spec-to-code compliance, differential review,
variant analysis, false-positive check, triage) ran over `main` at `89a9b2b`.
The run was static: no build, no tests, no transactions, plus read-only
mainnet RPC reads. It produced 63 candidates. The false-positive pass
confirmed 21 as real defects and refuted 28; the rest were duplicates or
design commentary. Triage accepted one as reachable by an outsider (C-DR-1)
and kept one open for lack of a second integrator (C-SE-2). The other
confirmed defects need the owner's own action, or are documentation.

The skills' working notes stay with the operator; this page is the public
record. Prior findings from `redteam-2026-11-13.md` were deduplicated, not
re-reported.

## What reaches mainnet

| Component | Fix live? |
|---|---|
| Falsara gateway (`services/falsara-near`, Vercel project `falsara-near`) | C-DR-1: yes, production deployment `dpl_5pCAXqjiFNSwd45bUtjy4ganeDyP`, built from `f786529` on 2026-11-14. C-VA-6: on `main`, live with the next gateway deploy |
| Hosted demo (`demo/live`) | C-SE-2 (demo side): yes, deployed from `e22b175`. C-VA-5 changes `setup.mjs` only, which runs on an operator machine |
| Policy wallet and wallet factory | No. `tp-wallet.hlpok1lad.tg`, the factory `triagepay.hlpok1lad.tg` and the wallets it created have no access keys and no upgrade method. The source fixes reach mainnet only through a new factory account (about 2.6 NEAR of storage) and new wallets |

## Reachable by an outsider

| ID | Finding | Fix | Test |
|---|---|---|---|
| C-DR-1 | The gateway's JavaScript verifier read only receipt logs. An `ft_transfer_call` of the route price to the code-less `gateway.hlpok1lad.tg` logs the outgoing NEP-141 event, then fails in `ft_on_transfer`, and native USDC refunds it. The gateway counted that as paid, so any account could get a paid check for gas | `lib/verify.js` follows the Rust verifier (`crates/near-adapter/src/verify.rs`): the token receipt's body must be `ft_transfer` with exactly 1 yoctoNEAR; any `ft_transfer_call` never pays; a receipt with two matching events is refused; the transaction must end in `SuccessValue`; a payment landing at the expiry second is late | `a refunded ft_transfer_call never pays (ToB C-DR-1)`, `the token receipt must be an ft_transfer with exactly 1 yoctoNEAR`, `the JS verifier agrees with the Rust fixtures` (the Rust fixture set, run through the JS verifier) |

## Owner-triggered or integration defects

| ID | Finding | Fix | Test |
|---|---|---|---|
| C-SC-4, C-VA-2 | Two seams reset an agent's daily total. Adding a second token, then `set_token_limits` on the first, started a fresh window. Dropping back to one token, then `set_agent` or `set_token_limits`, discarded the per-token spend. Either allowed up to 2 x `daily_max` in one day against a stolen agent key, after an owner action | Limits and spend live only in per-token budgets keyed by the token id, on one-token wallets too. Allowlist changes never move them. A removed token's budget is kept while it carries spend today | `adding_a_token_does_not_reset_the_daily_cap`, `dropping_to_one_token_does_not_reset_the_daily_cap`, `a_removed_tokens_spend_survives_the_day_it_was_spent`, `a_removed_token_with_no_spend_today_is_pruned`, `switching_the_single_token_does_not_carry_limits_across_units` |
| C-SE-1 | `set_token_limits` set `active = true`, so tightening a revoked agent's limits silently restored its ability to pay | `set_token_limits` leaves `active` alone; `set_agent` remains the documented way back | `set_token_limits_does_not_reactivate_a_revoked_agent`, `set_agent_still_reactivates`; sandbox `factory.rs` revoke step |
| C-ACB-2 | Multi-token `set_agent` stored any number of payees; the agent was then unusable | Both branches refuse more than 32 payees (`ERR_INVALID_POLICY`) | `multi_token_set_agent_refuses_too_many_payees` |
| C-SC-6 | Beta custody rose on deposits and fell only on withdrawals, so after the agent paid out, honest deposits could be refused below the real cap | A settled payment lowers custody | `a_settled_payment_frees_deposit_headroom`, `a_failed_payment_leaves_custody_alone` |
| C-SE-4 | `create_wallet` deployed whatever the committed blob inflated to; `WALLET_CODE_SHA256` was checked by a unit test only | `create_wallet` refuses unless the inflated bytes match `WALLET_CODE_LEN` and `WALLET_CODE_SHA256`. The embedded wallet is rebuilt from the fixed source with the CI recipe: 219,653 bytes, sha256 `cb4f94697d30bf18b3f887694ec78b9eeb6992a508e02ea4c3cb39b212fc5974` | `embedded_code_is_the_optimized_wallet`, `the_digest_check_refuses_other_bytes` |
| C-SE-2 | `pay` returns a payment id as a value when it queues a hold and a promise when it transfers; the interface doc said "Returns the promise", and the demo labelled a held Falsara payment as paid | The interface table states both shapes and that call success is not payment. The demo no longer calls a held payment paid (`e22b175`) | Demo test for held Falsara payments |
| C-VA-5 | `demo/live/setup.mjs` wrote the wallet it had just deployed into `verifiedCodeHashes`, so a re-run with modified wasm certified itself | The reviewed list is kept; a new hash joins only when `TRIAGEPAY_TRUST_CODE_HASH` names that exact hash | Manual: `setup.mjs` refuses a mismatched hash |
| C-VA-6 | The gateway's `/health` echoed the core's self-reported gate pins | The gateway pins the expected versions (`EXPECTED_CORE_PINS`, override `FALSARA_EXPECTED_PINS`) and answers 503 with the drift when the core's report differs | `a core whose self-reported pins drift is not healthy (ToB C-VA-6)` |

## Documentation defects

| ID | Finding | Fix |
|---|---|---|
| C-SC-1 | README, `policy-wallet.md` and the claims ledger said one token per wallet; factory wallets allow 8 | Corrected, with which build runs where |
| C-SC-2 | `review-policy.md` said a hold reserves the day and a reject restores it; builds from `94e62f1` on check without reserving | Corrected; `tp-wallet` (from `ef7472c`) does reserve, and the page says so |
| C-SC-3 | The interface table omitted 11 public methods, including the review queue, per-token limits and `expire_holds` | Regenerated from HEAD with events and error codes |
| C-SC-5 | The claims ledger marked "One token per wallet" GREEN and cited two tests that did not exist | Row replaced; `scripts/claims-check.py` fails CI when any cited test name does not resolve |

## Kept as is

| ID | Finding | Why |
|---|---|---|
| C-SE-12, C-TIA-4 | Caps are unitless integers; a wNEAR-sized number on USDC is 10^18 too large | Owner input; the contract cannot know which decimals the owner meant. Budgets are now keyed by token, so a number is never re-read in another token's unit |
| C-SE-3, C-TIA-3 | The proof trusts the token's event amount; settlement trusts "promise did not fail" | Documented residuals of choosing a token; only owner-allowlisted tokens are paid |
| C-TIA-6 | Anyone can fill the beta custody headroom with an `ft_transfer_call` donation | The griefer pays with real tokens and one owner `withdraw` reverses it |
| C-VA-3 | The demo prices every token at 24 decimals | Display and proposal only; the contract enforces atomic units |
| C-DR-6 | A memo that fails the HMAC answers `proof_already_consumed` | Error label only; no security effect |
