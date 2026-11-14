# Policy wallet: build, toolchain and artifacts

The contract started as a pre-window spike of plan prompt 02 (spend-policy
wallet contract), built outside the submission repository under ADR-0006
option C and imported with its history. It landed in this layout inside the
hackathon window (2026-11-04). It is deployed on mainnet: the review-queue
build at `tp-wallet.hlpok1lad.tg` (2026-11-13, built from `ef7472c`) and the
per-account copies the factory `triagepay.hlpok1lad.tg` deploys (built from
`main` at `1b1a578`). The deployed contracts have no access keys and no
upgrade method, so a fix to this source reaches mainnet only as a new
deployment; the evidence for each deployment lists its source commit.

## Layout

| Path | What |
|---|---|
| `crates/policy-core/` | Dependency-free policy arithmetic (proptest is a dev-dependency only); a member of the root workspace |
| `contracts/` | Its own Cargo workspace (`contracts/Cargo.toml`, `contracts/Cargo.lock`); the root workspace lists it under `exclude` |
| `contracts/policy-wallet/` | The wallet contract (near-sdk `=5.29.1`), depending on `../crates/policy-core` by path |
| `contracts/test-token/` | Minimal NEP-141 token for sandbox tests only |
| `contracts/tests/` | near-workspaces sandbox integration tests (Linux-only) |
| `contracts/.cargo/config.toml` | Windows NASM switch (see below); applies to cargo runs inside `contracts/` |
| `.github/workflows/contracts.yml` | Linux CI: fmt, clippy, both wasm builds with size and SHA-256, unit and sandbox tests |
| `docs/decisions/0008-spend-policy-wallet.md` | ADR |
| `docs/contracts/policy-wallet.md` | Interface, events, error codes, threats, testnet evidence |
| `docs/contracts/mainnet-deployment-plan.md` | Mainnet plan (not executed) |

The root workspace never sees near-sdk: only `policy-core` is shared, and
it has no runtime dependencies. All commands below run from `contracts/`
unless a subdirectory is named.

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
the research note on contract toolchains. Deployability is proven twice:
the Linux CI sandbox run (18 tests, 2026-11-12) and the testnet deployments
of 2026-11-06 and 2026-11-11.

Working build command (run from `contracts/policy-wallet/`; the artifact
lands in the workspace target directory, `contracts/target/near/`):

```
cargo near build non-reproducible-wasm --locked --no-abi \
  --env "RUSTFLAGS=--remap-path-prefix=<cargo home>=/cargo --remap-path-prefix=<rustup home>=/rustup --remap-path-prefix=<repository root>=/src"
```

The three placeholders are the absolute paths of the build host's cargo
home, rustup home and repository checkout (in Git Bash on Windows, convert
them with `cygpath -w`). Use `CARGO_HOME` and `RUSTUP_HOME`, not
`$HOME/.cargo`: on this image those are `/usr/local/cargo` and
`/usr/local/rustup`. The factory's embedded wallet also remaps those two
absolute paths and the `contracts/` directory. The remapping is required
for any wasm that is deployed; see "Host paths in the wasm" below.
cargo-near force-appends `--cfg near` to these flags.

The wallet bytes inside the factory are `contracts/wallet-factory/res/policy_wallet.wasm.zlib`.
At HEAD, inflated, that is 219,653 bytes, sha256
`cb4f94697d30bf18b3f887694ec78b9eeb6992a508e02ea4c3cb39b212fc5974`:
the wallet with the 2026-11-14 Trail of Bits fixes, rebuilt on this host
with the CI recipe (the remaps above, then binaryen `wasm-opt` version 108
`-Oz --strip-debug --strip-producers --enable-bulk-memory
--enable-nontrapping-float-to-int --enable-sign-ext`, then zlib level 9).
`create_wallet` refuses to deploy bytes whose length or SHA-256 differ from
`WALLET_CODE_LEN` and `WALLET_CODE_SHA256`. This build is not deployed.

The deployed factory `triagepay.hlpok1lad.tg` embeds the previous blob
(introduced at `94e62f1`, byte-identical at `1b1a578`): 221,884 bytes,
sha256 `17acbefc65151683d5a37771b785ec0e5a9fa6debfbdc566084370a714e25b7e`,
which is the code hash (`2bR9xosy...`) of the wallets it created.
`scripts/mainnet/budget.mjs` keeps those numbers because it prices calls
to that live factory. The rest of this paragraph is about that blob.
The cargo registry folder `index.crates.io-<hash>` differs between machines
and is the same length, so a remap of only the cargo home still produces
two hashes. The build remaps that folder to `/cargo/registry/src` last,
because rustc keeps the last matching prefix. After that remap the size
matches and the hash does not: this host produced
`17acbefc65151683d5a37771b785ec0e5a9fa6debfbdc566084370a714e25b7e`, and
GitHub Actions produced
`48562c554fe8ec0d29a4aa10222eceec53d36954d10f9c7cea0f124a5992f41e`, both
221,884 bytes. The committed blob is the one from this host. CI rejects a
rebuild that still contains a host path or the registry folder name.

`--no-abi` is required on Windows: the ABI step builds a host `cdylib` with
`near-sdk/__abi-generate`, and the MSVC linker fails on the unresolved NEAR
host-function imports (`LNK2019`, 51 unresolved externals). On Linux the
default build (with embedded ABI) is expected to work; it would produce a
different hash because the ABI is embedded in the wasm.

### Risk 2: near-workspaces sandbox on native Windows

Result: not supported. near-sandbox ships Linux x86_64, Linux aarch64 and
macOS arm64 binaries only. The sandbox tests in `contracts/tests/` are written, gated
with `#![cfg(not(windows))]` so the crate compiles everywhere, and run only
in the Linux CI workflow (`.github/workflows/contracts.yml`). They cannot
run on this host; on Linux CI all 18 passed on 2026-11-12. On Windows they
are compiled and reported as ignored.

### Host build note: NASM

near-sdk's `unit-testing` feature pulls `aws-lc-sys`, whose Windows build
needs NASM. `contracts/.cargo/config.toml` sets `AWS_LC_SYS_PREBUILT_NASM=1`
so the crate uses its bundled prebuilt objects; the variable is ignored
elsewhere. Cargo reads it for any command run inside `contracts/`. The root
workspace does not need it: `policy-core` has no dependencies.

### Gate commands

From `contracts/`:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
(cd policy-wallet && cargo near build non-reproducible-wasm --locked --no-abi --env "RUSTFLAGS=...")
```

with the `RUSTFLAGS` value shown above.

`policy-core` is tested by the root gate (`cargo test --workspace` at the
repository root).

A plain `cargo build -p policy-wallet` fails by design: near-sdk 5.29 refuses
non-wasm builds without the `unit-testing` feature. `cargo clippy
--target wasm32-unknown-unknown` also fails, because dev-dependency features
pull proc-macro crates into the wasm graph; the host clippy run plus the
cargo-near wasm build cover the contract code.

## Build artifact

| Item | Value |
|---|---|
| File | `contracts/target/near/policy_wallet/policy_wallet.wasm` |
| Size | 174,618 bytes |
| SHA-256 | `baad3c689a3638b50602781aa62bc9e221643d7a437f19eecbd20d7968773130` |
| SHA-256 (base58, as cargo-near prints) | `DZi3U9vP4eeFCKoNAxGMAxVk1isoAHRdKFu1xvBHbjm1` |
| Built | 2026-11-11 on the Windows build host, from the change that limits the token allowlist to one token (applied on top of commit `ff947af`), rustc 1.98.0, cargo-near 0.21.1, with the path remapping above. Deployed on testnet on 2026-11-11 for the agent demo (`docs/demo/transcript-2026-11-11.md`); the testnet run of 2026-11-06 used the previous build (table below) |
| Host-path scan | 0 matches for the build host's user, cargo, rustup or checkout paths; panic locations read `/src/...`, `/cargo/...`, `/rustup/...` |
| Imports | 37 functions, all from module `env` |
| Exports | `new`, `pay`, `on_pay_complete`, `set_agent`, `revoke_agent`, `set_tokens`, `pause`, `unpause`, `withdraw`, `on_withdraw_complete`, `close`, `on_close_check`, `ft_on_transfer`, `get_agent`, `get_spent`, `get_config`, `contract_source_metadata` |

Earlier builds. Only the first row was ever deployed, and only on testnet
(the 2026-11-06 run in `docs/contracts/policy-wallet.md`; that wallet
account was deleted by the run's last step):

| Build | Size | SHA-256 | Why it differs |
|---|---|---|---|
| Landed layout `e96f5d0`, with remapping, 2026-11-06 (base58 `9Ebz6ABpWwNkjgst6YoZ48yvkYcbcowRYiBK186Lt9RB`) | 174,583 | `7a599a50b37fc25961da3e0e2c97a9108eb483ef1b591d1506e08e44bf13a6a2` | Allowed up to 16 tokens with one shared set of limits; superseded by the one-token rule |
| Spike, commit `a6b5d8b` (before `close`) | 165,025 | `0e1b5272d3c03f24f3272d19fddc46751ef7aab9449a0b1295e3798ba38408fd` | No `close` |
| Spike, commit `e1aecff` | 174,798 | `22de525576ea9f8711c8db59cdee6ad06cf221d5f6f818a4499d989d29743c9c` | Host paths embedded (see below) |
| Landed layout `e96f5d0`, no remapping | 174,803 | `5ccf9e1bfb0f2c4e8dbca9434976e305e99643dd519c2ee3040e7a84d458f887` | Host paths embedded; the 5-byte difference from the spike build was not investigated, because both builds are superseded |

### Host paths in the wasm

Panic locations compiled into the wasm carry the source path of every
crate that can panic. Without remapping, a Windows build embeds the build
host's cargo registry path and rustup toolchain path, both under the
user's profile folder; the spike's note that "the host path is not
embedded" held only for the checkout directory. A deployed contract is
public, so those strings would be published on chain. The remapped build
above contains no string with a user profile, drive letter or checkout
path (checked by scanning the module's printable strings); registry paths
appear as `/cargo/registry/src/...` and standard-library paths as
`/rustup/...`. The CI workflow applies the same remapping and fails the
job if the runner's home path survives in either wasm.

This is a non-reproducible build: a different rustc or cargo-near version,
or another operating system (path separators differ), changes the hash.
The reproducible-build path (`cargo near build reproducible-wasm` with a
pinned Docker image recorded in `[package.metadata.near.reproducible_build]`)
is open; the image tag and digest must be taken from the cargo-near release
notes, never guessed.

## Test counts (this host)

| Suite | Count | Runs on Windows |
|---|---|---|
| policy-core unit tests | 25 | yes |
| policy-core property tests (proptest, 256 cases each) | 3 | yes |
| policy-wallet unit tests (near-sdk mocked runtime) | 65 | yes |
| test-token unit tests | 3 | yes |
| sandbox integration tests (near-workspaces) | 18 | no: compiled and reported as ignored on Windows; 18 passed on Linux CI (2026-11-12) |

Test-token wasm (spike build, same command without remapping, from
`contracts/test-token/`): 148,419 bytes, SHA-256
`1085d171aac31acbc4e6005fa3eebd16ae2bf7cf26c1d80439495aa57e61a188`. The test
token is never deployed to a public network; its CI build is remapped like
the wallet's. It is built from the crates.io release of `near-contract-standards` `=5.29.1`
(locked in `Cargo.lock`) rather than from a git checkout of the near-sdk
fungible-token example; the example's code shape is reproduced in
`contracts/test-token/src/lib.rs`.

Hash caveat: a Linux CI build prints a different SHA-256 than the Windows
build above (path separators and toolchain names differ even after
remapping). The CI log hash is the reference for CI-built artifacts; the
mainnet hash must come from the reproducible build.

## Deviations from prompt 02

- `on_pay_complete` takes `(agent_id, token, receiver_id, amount,
  payment_id, window_index)` instead of `(agent, amount, payment_id)`. The
  window index makes a release after a UTC-midnight rollover a no-op
  instead of subtracting from the new day; token and receiver complete the
  `pay_settled` / `pay_failed` events.
- One token per wallet (2026-11-11). The token allowlist holds at most one
  token, not sixteen. Limits and the daily total are plain integers in the
  token's atomic units and are not kept per token, so a second token with
  different decimals could be moved up to the same integers
  (`limits_sized_for_one_token_cannot_move_another`).
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
