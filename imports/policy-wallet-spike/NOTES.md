# Policy-wallet spike notes

Pre-window spike of plan prompt 02 (spend-policy wallet contract), built
outside the submission repository under ADR-0006 option C. Nothing here has
been deployed or signed on any network.

## Layout

| Path | What |
|---|---|
| `policy-core/` | Dependency-free policy arithmetic (proptest is a dev-dependency only) |
| `contracts/policy-wallet/` | The wallet contract (near-sdk `=5.29.1`) |
| `contracts/test-token/` | Minimal NEP-141 token for sandbox tests only |
| `tests/` | near-workspaces sandbox integration tests (Linux-only) |
| `docs/` | ADR-0008 draft, interface doc, mainnet deployment plan |

The spike uses one Cargo workspace for convenience. When this lands in the
submission repository, `policy-core` joins the root workspace as
`crates/policy-core` and everything under `contracts/` (plus the sandbox
tests) moves into its own `contracts/` workspace, so the root workspace
never sees near-sdk.

## Toolchain (verified on the build host, 2026-10-21)

| Item | Value |
|---|---|
| rustc / cargo | 1.98.0 / 1.98.0, stable, no `rust-toolchain.toml` pin |
| wasm target | `wasm32-unknown-unknown` |
| cargo-near | 0.21.1 |
| near-sdk | `=5.29.1` (MSRV 1.93; declares minimum protocol version 84) |
| near-contract-standards | `=5.29.1` (test token only) |
| near-workspaces | `=0.23.0` (sandbox tests only) |

### Risk 1: wasm features emitted by recent rustc

Result: no pin needed. cargo-near 0.21.1 runs a `wasm-opt -O` post-step with
`--enable-nontrapping-float-to-int --enable-bulk-memory` and documents that
step as required for rustc >= 1.82. The built module has a DataCount
section (bulk memory), which current mainnet protocol versions accept per
the research note on contract toolchains. Deployability is still unproven
on this host because no sandbox runs here (see risk 2); the first Linux CI
sandbox run is the deployability proof.

Working build command (run from `contracts/policy-wallet/`):

```
cargo near build non-reproducible-wasm --locked --no-abi
```

`--no-abi` is required on Windows: the ABI step builds a host `cdylib` with
`near-sdk/__abi-generate`, and the MSVC linker fails on the unresolved NEAR
host-function imports (`LNK2019`, 51 unresolved externals). On Linux the
default build (with embedded ABI) is expected to work; it would produce a
different hash because the ABI is embedded in the wasm.

### Risk 2: near-workspaces sandbox on native Windows

Result: not supported. near-sandbox ships Linux x86_64, Linux aarch64 and
macOS arm64 binaries only. The sandbox tests in `tests/` are written, gated
with `#![cfg(not(windows))]` so the crate compiles everywhere, and run only
in the Linux CI workflow (`.github/workflows/contracts.yml`). They have not
been executed on this host; treat them as unverified until a Linux run.

### Host build note: NASM

near-sdk's `unit-testing` feature pulls `aws-lc-sys`, whose Windows build
needs NASM. `.cargo/config.toml` sets `AWS_LC_SYS_PREBUILT_NASM=1` so the
crate uses its bundled prebuilt objects; the variable is ignored elsewhere.

### Gate commands

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cd contracts/policy-wallet && cargo near build non-reproducible-wasm --locked --no-abi
```

A plain `cargo build -p policy-wallet` fails by design: near-sdk 5.29 refuses
non-wasm builds without the `unit-testing` feature. `cargo clippy
--target wasm32-unknown-unknown` also fails, because dev-dependency features
pull proc-macro crates into the wasm graph; the host clippy run plus the
cargo-near wasm build cover the contract code.

## Build artifact

| Item | Value |
|---|---|
| File | `target/near/policy_wallet/policy_wallet.wasm` |
| Size | 174,798 bytes |
| SHA-256 | `22de525576ea9f8711c8db59cdee6ad06cf221d5f6f818a4499d989d29743c9c` |
| SHA-256 (base58, as cargo-near prints) | `3M7V5UVy1QpmbFk1BzzcDGA9nsMM5zGMP5gDG56bY7Ab` |
| Built at commit | `e1aecff` (adds `close`; the pre-`close` build at `a6b5d8b` was 165,025 bytes, SHA-256 `0e1b5272d3c03f24f3272d19fddc46751ef7aab9449a0b1295e3798ba38408fd`) |
| Imports | 37 functions, all from module `env` |
| Exports | `new`, `pay`, `on_pay_complete`, `set_agent`, `revoke_agent`, `set_tokens`, `pause`, `unpause`, `withdraw`, `on_withdraw_complete`, `close`, `on_close_check`, `ft_on_transfer`, `get_agent`, `get_spent`, `get_config`, `contract_source_metadata` |

The same hash came out of two builds from different checkout directories,
so the host path is not embedded in the module. This is a non-reproducible
build: a different rustc or cargo-near version can change the hash. The
reproducible-build path (`cargo near build reproducible-wasm` with a pinned
Docker image recorded in `[package.metadata.near.reproducible_build]`) is
open; the image tag and digest must be taken from the cargo-near release
notes, never guessed.

## Test counts (this host)

| Suite | Count | Runs on Windows |
|---|---|---|
| policy-core unit tests | 25 | yes |
| policy-core property tests (proptest, 256 cases each) | 3 | yes |
| policy-wallet unit tests (near-sdk mocked runtime) | 62 | yes |
| test-token unit tests | 3 | yes |
| sandbox integration tests (near-workspaces) | 18 | compiled and reported as ignored on Windows; written for Linux CI, never executed yet |

Test-token wasm (same build command, from `contracts/test-token/`): 148,419
bytes, SHA-256
`1085d171aac31acbc4e6005fa3eebd16ae2bf7cf26c1d80439495aa57e61a188`. It is
built from the crates.io release of `near-contract-standards` `=5.29.1`
(locked in `Cargo.lock`) rather than from a git checkout of the near-sdk
fungible-token example; the example's code shape is reproduced in
`contracts/test-token/src/lib.rs`.

Hash caveat: a non-reproducible build can embed dependency source paths
from the build host's cargo registry, so a Linux CI build may print a
different SHA-256 than the one above. The CI log hash is the reference for
CI-built artifacts; the deployed hash must come from the reproducible
build.

## Deviations from prompt 02

- `on_pay_complete` takes `(agent_id, token, receiver_id, amount,
  payment_id, window_index)` instead of `(agent, amount, payment_id)`. The
  window index makes a release after a UTC-midnight rollover a no-op
  instead of subtracting from the new day; token and receiver complete the
  `pay_settled` / `pay_failed` events.
- Extra events beyond the prompt's list: `tokens_set` (owner config change
  is auditable), `funded` (`ft_on_transfer`), and `withdraw_failed`.
  `withdrawn` is emitted from a callback on success only, so a failed
  withdrawal never logs `withdrawn`.
- Extra stable codes beyond the prompt's six: `ERR_ZERO_AMOUNT`,
  `ERR_INVALID_POLICY`, `ERR_OVERFLOW`, `ERR_RELEASE_MISMATCH` (core) and
  `ERR_NOT_OWNER`, `ERR_DEPOSIT_NOT_ZERO`, `ERR_INSUFFICIENT_GAS`,
  `ERR_MEMO_TOO_LONG`, `ERR_TOO_MANY_TOKENS`, `ERR_SELF_ACCOUNT` (contract).
- `new` is `#[private]` as well as `#[init]`: only the wallet account itself
  can initialize, so deploy and `new` must go in one batch transaction
  signed by the wallet account. This closes the front-running window
  between deploy and init.
- `pay` refuses a non-zero deposit (`ERR_DEPOSIT_NOT_ZERO`) and less than
  35 TGas prepaid (`ERR_INSUFFICIENT_GAS`), so the callback always has gas
  to release a reservation.
- `revoke_agent` keeps the entry (inactive) so the daily total survives a
  revoke-and-re-add inside one day; `set_agent` on an existing agent keeps
  its spend window for the same reason.
- `#[private]` cannot be exercised in near-sdk unit tests (the generated
  wasm entry point enforces it and the mocked runtime bypasses that); the
  sandbox tests cover it.
- Added after lead review: owner-only `close(beneficiary)` (1 yocto,
  paused only) with the private `on_close_check` callback and the
  `closed` / `close_refused` events and `ERR_NOT_PAUSED`, so the keyless
  wallet's storage stake is recoverable without an upgrade path.
