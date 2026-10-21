# NEAR contract toolchain (research, 2026-10-21)

Scope: prompt 02 research items 1-5 (SDK, build, sandbox, access keys, NEP-141/145/297,
callbacks, gas, storage price). Facts already verified in `docs/integrations/near.md`
(RPC protocol, settlement asset, testnet faucet, borsh field order, self-transfer topology) are
not repeated here.

Status legend: **Verified-primary** = read from the official repository, spec, docs, or a live
read-only RPC response. **Secondary** = third-party or indirect. **Unverified** = not confirmed;
the row says what would confirm it. All access dates are 2026-10-21.

## 1. Versions and the build toolchain

| Claim | Source | Accessed | Status |
|---|---|---|---|
| Latest stable `near-sdk` is **5.29.1** (published 2026-09-17). `5.30.0-rc.1` (2026-10-19) is a release candidate. | https://crates.io/api/v1/crates/near-sdk ; https://github.com/near/near-sdk-rs/releases | 2026-10-21 | Verified-primary |
| `near-sdk` 5.29.1 declares `rust-version = "1.93"` (workspace `Cargo.toml`). 5.30.0-rc.1 bumps it to 1.95 ("bump rust-version to 1.95", #1639). | https://raw.githubusercontent.com/near/near-sdk-rs/near-sdk-v5.29.1/Cargo.toml ; crates.io API | 2026-10-21 | Verified-primary |
| `near-sdk` 5.29.1 declares `[package.metadata.near] min_protocol_version = 84`; the comment says PV 84 (nearcore 2.12) "unlocks bulk-memory + reftypes opcodes, enabling rustc 1.93+". | https://raw.githubusercontent.com/near/near-sdk-rs/near-sdk-v5.29.1/near-sdk/Cargo.toml | 2026-10-21 | Verified-primary |
| `near-sdk` 5.30.0-rc.1 declares `min_protocol_version = 88` (nearcore 2.14, "unconditional protocol 87 imports"). | https://raw.githubusercontent.com/near/near-sdk-rs/near-sdk-v5.30.0-rc.1/near-sdk/Cargo.toml | 2026-10-21 | Verified-primary |
| Live mainnet runs **protocol 86**, nearcore 2.13.4. Live testnet runs **protocol 88**, nearcore 2.14.0-rc.5. | read-only `status` RPC on https://rpc.mainnet.near.org and https://rpc.testnet.near.org | 2026-10-21 | Verified-primary |
| `near-contract-standards` is versioned with `near-sdk` (same workspace); its `FungibleToken` lives in `near-contract-standards/src/fungible_token/`, and the repository ships `examples/fungible-token` (contract `ft` plus a `test-contract-defi` receiver). | near-sdk-rs repository tree at tag `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary |
| Latest stable `cargo-near` is **0.22.0** (2026-07-13); 0.23.0-rc.1/rc.2 are pre-releases. The locally installed 0.21.1 (2026-06-03) supports nearcore 0.36 / 2.12 / protocol 84 (#427). | https://github.com/near/cargo-near/releases ; https://raw.githubusercontent.com/near/cargo-near/main/CHANGELOG.md | 2026-10-21 | Verified-primary |
| `cargo near build non-reproducible-wasm` is "a thin wrapper around" `cargo build --target wasm32-unknown-unknown --release`; `reproducible-wasm` builds inside Docker from committed source, requires a committed `Cargo.lock` and a clean tree, and for deploy the code must be pushed to `package.repository`. | https://raw.githubusercontent.com/near/cargo-near/main/README.md | 2026-10-21 | Verified-primary |
| Reproducible-build metadata lives in `[package.metadata.near.reproducible_build]`: `image`, `image_digest`, `container_build_command` (for images from `0.13.0-rust-1.83.0` onward the base is `["cargo","near","build","non-reproducible-wasm","--locked"]`), optional `passed_env`, and named `variant.<name>` tables. | same README | 2026-10-21 | Verified-primary |
| cargo-near always appends `--cfg near` to wasm rustflags (near-sdk 5.27+ uses it) and strips ambient `RUSTFLAGS` / `CARGO_ENCODED_RUSTFLAGS`. | same README; CHANGELOG 0.20.2 (#417) | 2026-10-21 | Verified-primary |
| Windows: cargo-near releases ship x64 and ARM64 Windows binaries and a PowerShell installer (`cargo-near-installer.ps1`). Reproducible builds additionally need Docker. | release page and README | 2026-10-21 | Verified-primary |
| The required wasm target is `wasm32-unknown-unknown` (`rustup target add wasm32-unknown-unknown`). | https://docs.near.org/smart-contracts/quickstart | 2026-10-21 | Verified-primary |

### rustc / wasm-feature compatibility (the 1.86 ceiling)

| Claim | Source | Accessed | Status |
|---|---|---|---|
| rustc 1.87+ emits bulk-memory and nontrapping-float-to-int opcodes for wasm; pre-2.12 nearcore VMs reject them. cargo-near therefore historically capped rustc at **1.86**. | cargo-near PR #425 (https://github.com/near/cargo-near/pull/425); `cargo-near-build/src/near/build/mod.rs` on `main` | 2026-10-21 | Verified-primary |
| Since cargo-near **0.20.3** the ceiling is dynamic: cargo-near reads `min_protocol_version` from the resolved `near-sdk` (transitive dependencies included). PV >= 84 means **no rustc ceiling**; absent or < 84 means max rustc 1.86. Constant: `BULK_MEMORY_PROTOCOL_VERSION: u32 = 84`. | same file and PR; CHANGELOG 0.20.3 | 2026-10-21 | Verified-primary |
| For rustc >= 1.87 cargo-near runs `wasm-opt` with `--enable-nontrapping-float-to-int --enable-bulk-memory` (`MIN_RUSTC_EMITTING_BULK_MEMORY_OPCODES = 1.87.0`). | `cargo-near-build/src/near/build/mod.rs` | 2026-10-21 | Verified-primary |
| An escape hatch `skip_rust_version_check` exists (described as an "escape hatch" for raw-WASM or edge cases); it prints a warning. | PR #425; CHANGELOG 0.20.2 (#420) | 2026-10-21 | Verified-primary |
| Mainnet runtime config (protocol 86) reports `reftypes_bulk_memory: true` and `vm_kind: "Wasmtime"`; nearcore 2.12.0 changelog: "The contract runtime now allows for bulk memory instructions in Wasm code." | `EXPERIMENTAL_protocol_config` (finality final) on mainnet; https://raw.githubusercontent.com/near/nearcore/master/CHANGELOG.md | 2026-10-21 | Verified-primary |
| **Conclusion for this host (cargo-near 0.21.1, rustc 1.98.0):** with `near-sdk = "=5.29.1"` the dynamic check finds PV 84, applies no ceiling, and enables the wasm-opt features; the output targets a VM that mainnet (PV 86) and testnet (PV 88) both run. No `rustup` pin to 1.86 is needed. | derived from the rows above | 2026-10-21 | Verified-primary (inputs); the build itself was not executed by this research |
| A contract whose dependency graph resolves an older `near-sdk` (before the `min_protocol_version` metadata was added) still gets the 1.86 ceiling and fails with rustc 1.98. | PR #425 ("existing contracts stay capped at 1.86") | 2026-10-21 | Verified-primary |
| **Do not use `near-sdk` 5.30.0-rc.1 for mainnet yet:** it targets PV 88 while mainnet is on PV 86. A contract that imports protocol-87/88 host functions may deploy or run on testnet and fail on mainnet. | derived from the two `min_protocol_version` rows and the live `status` rows | 2026-10-21 | Likely (inference); verify by building with 5.30.0-rc.1 and listing wasm imports, or by deploying to a 2.13.4 sandbox |

## 2. near-workspaces (sandbox) and Windows

| Claim | Source | Accessed | Status |
|---|---|---|---|
| Latest stable `near-workspaces` is **0.23.0** (2026-07-09, `rust-version` 1.93); 0.24.0-rc.1/rc.2 (rust 1.95) are pre-releases. | https://crates.io/api/v1/crates/near-workspaces | 2026-10-21 | Verified-primary |
| `near-workspaces` 0.23.0 depends on `near-sandbox = "0.3.12"` (semver-compatible with 0.3.16, the latest stable); `main` (0.24.0-rc.2) depends on `near-sandbox = "0.4.0-rc.1"`. | https://raw.githubusercontent.com/near/near-workspaces-rs/near-workspaces-v0.23.0/workspaces/Cargo.toml ; crates.io near-sandbox | 2026-10-21 | Verified-primary |
| README platform statement: "MacOS (x86 and M1) or Linux (x86) for sandbox tests." No Windows mention. | https://raw.githubusercontent.com/near/near-workspaces-rs/main/README.md | 2026-10-21 | Verified-primary |
| **The sandbox does not run on native Windows.** `near-sandbox` v0.3.16 `platform()` returns a binary only for `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64` (Darwin-x86_64 "is not supported for some time now"); every other target returns `None` and the download fails with `UnsupportedPlatformError("only linux-x86_64, linux-aarch64, and darwin-arm64 are supported")`. The same code is on `main`. | https://raw.githubusercontent.com/near/near-sandbox-rs/v0.3.16/src/runner/mod.rs | 2026-10-21 | Verified-primary |
| `NEAR_SANDBOX_BIN_PATH` short-circuits the download and `SANDBOX_ARTIFACT_URL` overrides the URL, but no official `neard` Windows build is known, so these do not make native Windows work. | same file | 2026-10-21 | Verified-primary (env vars); "no Windows neard build" is Likely |
| Default sandbox node version: `near-sandbox` 0.3.16 = **2.13.4** (matches mainnet); `main` / 0.4.x = `2.14.0-rc.4` (testnet's line). | `src/lib.rs` at `v0.3.16` and `main` | 2026-10-21 | Verified-primary |
| Time travel: `worker.fast_forward(blocks)` advances height, timestamp and epoch. `near_workspaces::compile_project` needs the `unstable` feature. | near-workspaces README | 2026-10-21 | Verified-primary |
| Windows-workable options: run the sandbox tests under WSL2 (Linux-x86_64 target) or in Linux CI; keep the native-Windows default suite to pure-Rust unit and property tests (`policy-core`) plus `near-sdk` `unit-testing` tests that need no node. | derived | 2026-10-21 | Recommendation |

## 3. Function-call access keys

| Claim | Source | Accessed | Status |
|---|---|---|---|
| A function-call key has `receiver_id` ("The only contract which the key allows to call"), optional `method_names` (omitted or empty = any method), and optional `allowance` (gas budget; omitted = unlimited). Function-call keys "do not allow to attach NEAR tokens to the call". | https://docs.near.org/protocol/access-keys ; https://nomicon.io/DataStructures/AccessKey | 2026-10-21 | Verified-primary |
| Allowance pays gas and fees only, is decreased together with the account balance, and cannot be changed in place (delete and re-add the key). When exhausted, "the key becomes unusable". | same two sources | 2026-10-21 | Verified-primary |
| nearcore enforcement (tag 2.13.4, `verify_function_call_permission`): the transaction must have **exactly one action**, it must be a `FunctionCall`, its **deposit must be zero** (`DepositWithFunctionCall`), the transaction receiver must equal the key's `receiver_id` (`ReceiverMismatch`), and a non-empty `method_names` must contain the method (`MethodNameMismatch`). Multiple or non-function-call actions give `RequiresFullAccess`; insufficient allowance gives `NotEnoughAllowance`. | https://raw.githubusercontent.com/near/nearcore/2.13.4/runtime/runtime/src/verifier.rs ; `core/primitives/src/errors.rs` | 2026-10-21 | Verified-primary |
| `one_yocto_on_promise: true` (live on mainnet) lets a **contract** attach exactly 1 yoctoNEAR to an outgoing promise function call even when its own balance is zero; nearcore's tests state it does "NOT relax the rule that function-call access keys cannot attach any deposit to a transaction." | `runtime/near-vm-runner/src/logic/logic.rs` and `core/parameters/src/vm.rs` at 2.13.4; verifier tests on `master`; live protocol config | 2026-10-21 | Verified-primary |
| Consequence for prompt 02: an agent's function-call key can call `wallet.pay` with zero deposit; the wallet contract can attach the 1 yocto to `ft_transfer`. Owner methods guarded by `assert_one_yocto` are unreachable from any function-call key. | derived from the rows above | 2026-10-21 | Verified-primary (rules) |

## 4. NEP-141, NEP-145, NEP-297

| Claim | Source | Accessed | Status |
|---|---|---|---|
| NEP-141 (Final): `ft_transfer(receiver_id, amount, memo?)`; "Caller of the method must attach a deposit of 1 yoctoⓃ for security purposes"; `amount` is a decimal string of a u128. Same rule for `ft_transfer_call`. | https://github.com/near/NEPs/blob/master/neps/nep-0141.md | 2026-10-21 | Verified-primary |
| Reference `assert_one_yocto()` requires `attached_deposit == 1 yocto` with message "Requires attached deposit of exactly 1 yoctoNEAR". | `near-sdk/src/utils/mod.rs` at `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary |
| Reference `internal_transfer` rejects `sender_id == receiver_id` ("Sender and receiver should be different") and `amount == 0` ("The amount should be a positive number"); an unregistered account panics "The account {id} is not registered". | `near-contract-standards/src/fungible_token/core_impl.rs` at `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary |
| NEP-141 recommends that a token contract account hold no access keys ("To prevent the deployed contract from being modified or deleted"). Live: the native USDC contract account holds one FullAccess key, so it is upgradeable by its key holder. | NEP-141 text; read-only `view_access_key_list` on the USDC account | 2026-10-21 | Verified-primary |
| NEP-145 (Final): `storage_deposit(account_id?, registration_only?)`; `storage_balance_bounds` returns `{min, max}` (yocto strings); attaching `min` registers; deposits above `max` are refunded; `storage_unregister` / `storage_withdraw` need 1 yocto. | https://github.com/near/NEPs/blob/master/neps/nep-0145.md | 2026-10-21 | Verified-primary |
| Live: the mainnet USDC contract `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1` returns `storage_balance_bounds` = `{"min":"1250000000000000000000","max":"1250000000000000000000"}` (0.00125 NEAR per registration). | read-only `call_function` (finality final), block 220978199 | 2026-10-21 | Verified-primary |
| NEP-297: events are log lines with the `EVENT_JSON:` prefix followed by one JSON object with required `standard`, `version`, `event` and optional `data`; whitespace is allowed. Known limit: 16 KB of logs. | https://github.com/near/NEPs/blob/master/neps/nep-0297.md ; live `max_total_log_length: 16384`, `max_number_logs: 100` | 2026-10-21 | Verified-primary |
| `#[near(event_json(standard = "..."))]` on an enum with `#[event_version("x.y.z")]` per variant generates `emit()`, which logs `EVENT_JSON:{json}` with `event` in snake_case and the payload under `data`. | `near-sdk/src/lib.rs` docs at `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary |
| Use `U128` (string) rather than `u128` for amounts in event payloads, so JSON consumers do not lose precision. | NEP-141 amount convention | 2026-10-21 | Recommendation |

## 5. Callbacks, promise results, gas

| Claim | Source | Accessed | Status |
|---|---|---|---|
| Mark callbacks `#[private]`; a failed external call does not roll back the caller's state ("Your state will not rollback automatically"); the callback must undo changes; "Always reserve gas for the callback." Examples use `Gas::from_tgas(...)` with `.with_static_gas(...)`; the default unused-gas weight is 1. | https://docs.near.org/smart-contracts/anatomy/crosscontract | 2026-10-21 | Verified-primary |
| In 5.29.1 `env::promise_result` is `#[deprecated = "use promise_result_checked to prevent out-of-gas errors"]`; use `env::promise_result_checked(idx, max_len)` or the parameter attributes `#[callback_result] r: Result<T, PromiseError>` (with optional `max_bytes`) / `#[callback_unwrap]`. | `near-sdk/src/environment/env.rs` and `examples/callback-results/src/lib.rs` at `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary |
| The reference FT reserves `GAS_FOR_RESOLVE_TRANSFER = Gas::from_tgas(5)` for its own callback and forwards the remaining gas to `ft_on_transfer`. | `core_impl.rs` at `near-sdk-v5.29.1` | 2026-10-21 | Verified-primary |
| Measured on mainnet (tx `ZonAdEjgAUM4tBnCR3nJTxe9LHiv7KcsbqRZdp9dns1`, a direct `ft_transfer` on native USDC with 30 Tgas attached): the token receipt burnt **2.32 Tgas**; the whole transaction burnt **2.86 Tgas**. | read-only `EXPERIMENTAL_tx_status` | 2026-10-21 | Verified-primary |
| Gas budgeting for `pay` (proposal): attach about 50 Tgas to the agent's `pay`; give the `ft_transfer` promise a static 10-15 Tgas (4-6x the measured burn) and the callback a static 10 Tgas. Measure in sandbox and record. | derived | 2026-10-21 | Recommendation; Unverified until measured |
| Live gas prices: `gas_price` RPC = 100 000 000 yocto/gas (0.0001 NEAR/Tgas); `min_gas_purchase_price` = 1 000 000 000 yocto/gas (0.001 NEAR/Tgas). Gas is prepaid at the purchase price and the unused part refunded, so the signer needs about 10x the burn cost available up front (the mainnet tx above was refunded 0.0306 NEAR from a 30 Tgas prepay). nearcore 2.13 changelog (NEP-642) describes this. | live `gas_price` and `EXPERIMENTAL_protocol_config`; nearcore CHANGELOG 2.13.0 | 2026-10-21 | Verified-primary |
| A function-call-key allowance is charged at the purchase price too, so allowances must be sized for prepaid gas, not burnt gas. | inferred from the purchase-price rule | 2026-10-21 | Likely; verify by reading the allowance before and after a testnet call |
| New-account creation charge: `account_creation_charge` = 0.007 NEAR (paid in gas). | live protocol config | 2026-10-21 | Verified-primary |

## 6. Storage staking price (live)

`EXPERIMENTAL_protocol_config` with `{"finality":"final"}` against https://rpc.mainnet.near.org
(protocol 86) returned `runtime_config.storage_amount_per_byte = "10000000000000000000"`
(1e19 yoctoNEAR per byte). Testnet returned the same value.

| Size | Locked NEAR |
|---|---|
| 1 byte | 0.00001 NEAR |
| 100 KB (100 000 bytes) | **1.0 NEAR** |
| 100 KiB (102 400 bytes) | 1.024 NEAR |
| 300 KB wasm (example) | 3.0 NEAR |

This matches the docs ("1E19 yoctoNEAR per byte, or 100kb per NEAR token",
https://docs.near.org/protocol/storage/storage-staking, accessed 2026-10-21). Accounts may use up
to 770 bytes without staking. Other live limits relevant to deployment: `max_contract_size`
4 194 304 bytes, `max_transaction_size` 1 572 864 bytes, `max_total_prepaid_gas` 1 000 Tgas.

## Implications for the build

- **Prompt 02:** pin `near-sdk = "=5.29.1"` and `near-contract-standards = "=5.29.1"`; do not adopt 5.30.0-rc.1 before mainnet reaches protocol 88. With that pin, cargo-near 0.21.1 accepts rustc 1.98.0 and no rustc 1.86 pin is required. Record rustc, cargo-near and the wasm SHA-256 in the CI log.
- **Prompt 02:** the near-workspaces sandbox will not start on native Windows. Run sandbox integration tests in WSL2 or Linux CI; pin `near-workspaces = "=0.23.0"` (sandbox node 2.13.4, same as mainnet). Native-Windows local runs cover `policy-core` and SDK unit tests only.
- **Prompt 02:** the function-call-key design holds: exactly one `FunctionCall`, zero deposit, receiver = wallet, `method_names = ["pay"]`. Owner methods with `assert_one_yocto` are unreachable from agent keys. Size allowances for the 0.001 NEAR/Tgas purchase price.
- **Prompt 02:** use `promise_result_checked` or `#[callback_result]`, never the deprecated `env::promise_result`. Release the reservation in the callback; state is not rolled back automatically.
- **Prompt 02 / 09:** deployment cost = wasm bytes x 1e19 yocto (1 NEAR per 100 KB), plus 0.00125 NEAR per USDC storage registration, plus 0.007 NEAR per new account.
- **Prompt 09:** budget the payer's liquid NEAR for prepaid gas at the purchase price (about 0.03 NEAR per 30 Tgas transaction, mostly refunded).
