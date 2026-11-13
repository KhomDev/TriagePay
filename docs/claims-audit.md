# Claims Audit

Every testable claim in the public documentation, mapped to its evidence, so
that a reviewer (human or AI) can check the repository against its own
promises without trusting prose. Audit date: 2026-11-11, taken on the tree
that adds the agent demo. Previous audit: 2026-10-21.

**Rule: if a claim is not green here, it is a defect: fix the doc or the
code, never the audit.**

Status words: GREEN (checked on the audit date), LIMITED (true only as
qualified in the row), OPEN (not proven; the docs must say so).

## Suites (regenerated 2026-11-11, not hand-edited)

The policy-core and policy-wallet rows below were updated on 2026-11-12
after those two suites were re-run. The other rows are still the
2026-11-11 audit.

| Suite | Count |
|---|---|
| aps-core | 23 |
| aps-payment | 9 |
| aps-api | 53 (api 12, concurrency 4, error_hygiene 7, near_flow 3, payment_flow 6, scan_flow 5, x402_flow 16) |
| aps-near | 58, plus 3 live read-only tests ignored by default |
| aps-auth | 25 |
| aps-store | 29 |
| policy-core | 29 (26 unit, 3 property). The new unit test was re-run on 2026-11-12; the other 28 are from the audit date |
| Rust workspace total | 226 passed, 3 ignored, counting that one new test on top of the 2026-11-12 workspace run |
| contracts: policy-wallet / test-token | 94 passed, 1 ignored / 3. The ignored test is the open review-queue availability issue ([PR #7](https://github.com/KhomDev/TriagePay/pull/7)). Re-run 2026-11-13 |
| contracts: sandbox | 23 tests. 18 passed on Linux CI on 2026-11-12 (https://github.com/KhomDev/TriagePay/actions/runs/43181541598); five more were added 2026-11-13 (hold/approve, hold/reject, revoke/restore, pay-only review methods, x402 receipt verifier). Not runnable on Windows |
| scanner | 319 |
| knowledge-compiler | 71, plus 1 skipped (local private-denylist scan) |
| triage-model | 73 (6 replay the committed cassettes) |
| root (OpenAPI, Python client) | 14 |
| ui | 17 |
| TypeScript client | 7 |

Also green on the audit date: `cargo fmt --check` and `clippy -D warnings`
in both workspaces, `ruff`, the ruleset byte-stability check, the voice
check, and the leakage tree scan (0 hits against the published list, and 0
hits against the operator's off-repo list).

## README.md

| Claim | Evidence | Status |
|---|---|---|
| Offline steps A: test count, 402 walkthrough, prepaid demo | Gate run; `scripts/x402-demo.py` and `scripts/demo.ps1` run on 2026-11-11, both exit 0 | GREEN |
| The agent's key is a function-call key limited to `pay` on the wallet | Transcript step `keys_view` (on-chain permission) | GREEN |
| Four wallet-paid x402 calls verified from the token's receipts | Transcript, section "The agent pays per call" | GREEN |
| The wallet refuses a non-allowlisted payee, an over-limit call and a fifth daily call | Transcript steps `injected_payment`, `over_per_call`, `scan:over-daily-limit`; unit tests `pay_to_non_allowlisted_payee`, `pay_over_per_call_limit`, `pay_over_daily_limit` | GREEN |
| The agent key cannot call owner methods | Transcript steps `self_escalation`, `self_escalation_no_deposit` (node rejections); unit `owner_with_two_yocto_is_refused` | GREEN |
| Revocation at the wallet stops the agent while its key still exists | Transcript step `pay_after_revoke` | GREEN |
| Only the token contract's own event funds an intent; other contracts' logs are ignored; `ft_transfer_call` is refused | `receipt_fixtures.rs` (21 tests) | GREEN |
| A wrong amount, a second payment or a late one becomes an orphan, never credit | `x402_flow.rs`, `provider_rpc.rs`; live check in the transcript | GREEN |
| The memo ends in a 128-bit random nonce | `intent_ids_end_in_a_128_bit_nonce_that_is_not_a_counter`, `requirement_memos_carry_an_unguessable_nonce` | GREEN |
| Money is integer-only; conservation holds after every operation | `money.rs`; `ledger_fuzz.rs` | GREEN |
| Development-only routes answer 403 unless the mock provider is active | `dev_only_routes_refuse_under_the_near_provider`; live check in the transcript | GREEN |
| One mainnet deposit verified on 2026-10-21 | `docs/integrations/near.md`; the capture is a fixture of the current verifier | LIMITED (verified live by the earlier argument-based verifier; the current verifier checks the same transaction from its recorded receipts) |
| Stage B runs against NEAR AI Cloud | 2026-11-12 spike, 50 calls, USD 0.096 (`docs/engine/model-selection.md`); paid route in `triage.v2` mode, live and from cassettes | GREEN |
| Stage B without a key, from recorded replies | `triage-model/tests/test_committed_cassettes.py` (6 tests) | GREEN |
| The sandbox tests pass on Linux | GitHub Actions run https://github.com/KhomDev/TriagePay/actions/runs/43181541598: 18 passed on 2026-11-12. The suite is now 23 tests | GREEN for the 2026-11-12 run; the five new tests run in the contracts workflow |
| No code predates the hackathon window | `git log --reverse`: first commit 2026-10-20; rules Q-001 count code from 2026-10-19 | GREEN |
| Hosted demo unit tests | `cd demo/live && npm test` | GREEN |
| Hosted demo model, policy apply, honeypot list | Code and unit tests in `demo/live`. A model call needs `NEARAI_API_KEY`. Apply needs `TRIAGEPAY_POLICY_APPLY=1`. The honeypot list is rebuilt from refused `pay` calls, not a contract registry | LIMITED |

## docs/protocols/x402-near.md

| Claim | Evidence | Status |
|---|---|---|
| Wire format and headers | `x402_flow.rs::payment_required_matches_the_v2_shape`; OpenAPI test | GREEN |
| Verification rules 1-6 | `receipt_fixtures.rs`, `provider_rpc.rs` | GREEN |
| Only an exact-amount confirmation is final for an intent | `a_failed_transaction_is_reported_but_never_closes_the_intent`, `a_wrong_amount_is_reported_but_never_closes_the_intent`, two flow tests, live check | GREEN |
| Single-use proof; replay returns the stored response | `pay_then_retry_settles_once_and_replays`, `a_concurrent_retry_storm_charges_once`; transcript `direct_payment`, `proof_reuse` | GREEN |
| Orphans are listed on `GET /payment/intents/{id}` | `an_expired_requirement_never_funds`; live check | GREEN (in memory: lost on restart) |
| Expired, unpaid requirements are dropped | `expired_requirements_nobody_paid_are_forgotten` | GREEN |
| Facilitator endpoints | `facilitator_supported_verify_and_settle`, `facilitator_rejects_mismatches_and_this_servers_own_intents` | LIMITED (stub RPC only; never used live; consumed proofs are kept in memory) |
| The profile is not upstream | Stated in the first line | GREEN |

## docs/contracts/

| Claim | Evidence | Status |
|---|---|---|
| Interface, events, error codes | 94 unit tests passed and 1 ignored, re-run 2026-11-13, including the review queue. The ignored test records the open availability issue from [PR #7](https://github.com/KhomDev/TriagePay/pull/7) and does not change the contract | GREEN |
| One token per wallet | `new_rejects_a_second_token`, `set_tokens_rejects_a_second_token`, `limits_sized_for_one_token_cannot_move_another` | GREEN |
| Testnet run of 2026-11-06, 28 steps | Evidence table in `policy-wallet.md`; step 12 re-read from an archival RPC on 2026-11-10 | GREEN for step 12; the other 27 rows are as recorded by the run |
| Current build on testnet | Transcript: deployed, initialized, key deleted in one batch; wasm SHA-256 in the evidence equals `build.md` | GREEN |
| The build is not reproducible | `build.md` says so; a third party cannot re-derive the hash from source | OPEN |
| Sandbox suite | 18 passed on Linux CI on 2026-11-12. The first run found three test bugs, no contract bug (see corrections). Five tests were added on 2026-11-13 | GREEN |
| The review-queue wallet is live on mainnet (`tp-wallet.hlpok1lad.tg`, code hash `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC`, built from `ef7472c`, deployed 2026-11-13) | `docs/demo/mainnet-campaign/2026-11-13`: deploy, `new` and key deletion in one batch (`J7yqf49G`), 0 access keys, held → approved → settled, held → rejected, on mainnet | GREEN |
| Graded mainnet tests on 2026-11-13: limits, malformed args, the pay-only key, a non-owner account, revoke and restore, pause, the review queue, 51 NEAR AI completions | `docs/demo/mainnet-campaign/2026-11-13/RESULTS.md`, one JSON file per step; `REGRADE.md` re-grades them with stricter rules | GREEN: 166 on-chain assertions hold and none fail. 4 LLM rounds where the model never tried to pay are not counted as passes (the first report counted them, 170). 6 stated values in recorded-only steps were checked afterwards and hold. It found one design issue: the agent can fill the review queue (OPEN, see that README) |
| The 2026-11-12 mainnet wallet (built from `82d64e3`, before the review queue) | Four mainnet transactions in `docs/demo/mainnet-e2e/2026-11-12` (paid, refused payee, revoke, revoked-agent refusal). I closed it on 2026-11-13 (`CX2Xd4Qk`) | GREEN as history; no longer deployed |
| I re-check mainnet every six hours, read-only, with no secrets | `.github/workflows/mainnet-watch.yml` runs `scripts/mainnet/live-check.mjs` on that schedule and on `workflow_dispatch`. It reads public fastnear RPC. It checks that `tp-wallet.hlpok1lad.tg` exists, that its code hash is `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC`, that it has no access keys, and that owner, pause flag, token `wrap.near`, agent `tp-agent2.hlpok1lad.tg` (active, per-call `1000000`, daily `4000000`, allowlist `[gateway.hlpok1lad.tg]`, pay-only key `ed25519:69i3HGAR51MzFSvGLjQqnZhk6Dpo39VGnaNU9LJscCdn`), and review (threshold `500000`, unknown payees held, queue capacity 8) match `scripts/mainnet/expected-live.json`. It does not pin today's spend or the key allowance. It checks the four proof transactions (`2RH77VLs…` paid, `HskzPv94…` refused, `CFutaZM9…` revoke, `C1BnCCrE…` inactive), the testnet wallet `tpd-e947e7-wallet.testnet` views, and `https://triagepay-demo.vercel.app/api/health` (`ok: true`) | GREEN |
| The review queue is live on testnet (`tpd-e947e7-wallet.testnet`): held, approved and settled, rejected | `docs/demo/testnet-review/2026-11-12` (EVIDENCE.json with balances before and after) | GREEN |
| The hosted agent `tpd-e947e7-agent.testnet` has one key, `pay` only | Cleanup transaction `74a2PEiV` and the `agentKeysAfter` list in the evidence | GREEN |
| Chain Signatures: one testnet signature that recovers to the derived address | `docs/contracts/chain-signatures.md`, `docs/demo/testnet-chainsig/2026-11-12/EVIDENCE.json`; SDK tests; ethers v6 cross-check | LIMITED (not broadcast; the request came from the owner account, not from the wallet contract) |

## docs/engine/

| Claim | Evidence | Status |
|---|---|---|
| Stage A gates are deterministic and ruleset-driven; unknown evaluators fail closed | `test_stage_a_golden`, `test_evaluators`, `test_ruleset` | GREEN |
| The combiner never raises a decision above Stage A | `test_combiner.py` (exhaustive table) | GREEN |
| Embedded instructions are flagged and never change a decision | Injection corpus tests; transcript finding 3; `test_flag_sees_through_emphasis_fullwidth_and_format_characters` | AMBER: the flag is a heuristic pattern list (now also over a folded view), and an obedient model can move `confidence_band` and `score`, though never the decision above Stage A |
| No accuracy figure is claimed | None appears in any document | GREEN |
| Stage B prompt, schema, retry and cache behaviour | `triage-model` tests with a fake model; the live spike | GREEN |
| The default model is TEE-verifiable | Catalogue flag recorded in `config.py`. The paid route does not fetch an attestation (ADR-0011). The hosted demo fetches a report and checks the nonce echo only ([attestation.md](engine/attestation.md)) | LIMITED |

## docs/architecture.md, threat-model.md, security-review.md, ADRs

| Claim | Evidence | Status |
|---|---|---|
| Component and trust-boundary description | Matches the crates listed in the README layout | GREEN |
| Threat rows name the test that covers them | Each named test exists (`git grep "fn <name>"`) | GREEN |
| Residual risks: in-memory ledger, one development caller, no rate limit, trusted RPC | `crates/api/src/lib.rs`, `main.rs` | GREEN (as stated limits) |
| ADR-0006: the window question is answered by rules Q-001 | Live rules page, read 2026-11-10 | GREEN |
| ADR-0011: attested verdicts are cut | No attestation code exists | GREEN |
| CI workflows | Both green on GitHub on 2026-11-12, after three fixes found by their first runs (see corrections) | GREEN |

## Corrections kept on record

| Date | Correction |
|---|---|
| 2026-10-21 | The function-call-key hardening claim was infeasible (such keys cannot attach the 1 yoctoNEAR `ft_transfer` needs); replaced by the policy wallet |
| 2026-10-21 | Published digests of private terms were reversible; real terms are now checked off-repo only |
| 2026-10-21 | The settlement asset is native USDC, not bridged USDC.e |
| 2026-11-06 | `rpc.mainnet.near.org` and `rpc.testnet.near.org` are deprecated; FASTNEAR is the default |
| 2026-11-11 | Review findings fixed: a hint could close someone else's requirement (failed transaction or one-unit transfer); memos were guessable; the direct deposit route minted credit under the NEAR provider; wallet limits were shared across tokens; scanner stderr reached 500 bodies; unpaid requirements were never pruned |
| 2026-11-11 | `wrap.testnet` cannot be the settlement asset: it emits no NEP-297 events. The x402 example and the testnet runs use an event-emitting token |
| 2026-11-11 | The README described the 2026-10-21 state; rewritten |
| 2026-11-12 | First GitHub CI run: `cargo audit` failed on RUSTSEC-2026-0459 (libsqlite3-sys via rusqlite 0.37) and RUSTSEC-2026-0384 (tokio 1.54.0); rusqlite moved to 0.41.0 and tokio to 1.54.1. The contracts workflow could not install cargo-near (missing libudev headers on the runner) |
| 2026-11-12 | First live Stage B calls: every request was refused (HTTP 400 for `uniqueItems` in the output schema and for a model that left the catalogue) or came back empty (reasoning used the whole 1024-token budget). Fixed in the client; the spike then ran clean |
| 2026-11-12 | First sandbox run: 3 of 18 failed on test bugs. `pay` with a deposit is refused by near-sdk's non-payable guard, so the contract's `ERR_DEPOSIT_NOT_ZERO` is not reached on chain; the overflow test funded the wallet before allowlisting the token; the close test measured the gas-paying owner as beneficiary |
| 2026-11-13 | The hosted demo can call NEAR AI when `NEARAI_API_KEY` is set. Without it the agent is scripted and labelled as such. A natural-language policy is previewed by a parser (or a schema-checked model). Applying it is off unless `TRIAGEPAY_POLICY_APPLY=1`. The honeypot list is not an on-chain registry |
| 2026-11-13 | `tp-wallet.hlpok1lad.tg` is no longer the 2026-11-12 pre-review build. The account now has code hash `45HbvWoxBiUoJssERUU6xr8EqAudAHcjS4kYG9NiDagC` (review-queue build from `ef7472c`), the agent is active, and the daily cap is `4000000`. The four proof transactions from 2026-11-12 still resolve with their original outcomes. I record the current views in `scripts/mainnet/expected-live.json` and do not pin spend or gas allowance |

## Open items

- Persistence, authentication and rate limiting are built as crates and not
  wired into the API.
