# Security Policy

## Reporting a vulnerability

Open a private security advisory on the repository, or contact the
maintainer directly. Please include reproduction steps and affected
commits. Do not open public issues for vulnerabilities.

## Scope

Everything in this repository: the policy-wallet and wallet-factory
contracts, the ledger core, the payment provider abstraction, the HTTP
API, the scan engine, the SDK, and the hosted demo (`demo/live`).

## Reviews

- **Red-team review, 2026-11-13.** Five independent reviewers plus a lead
  verification pass, static, on the tree at `27e9d2c`. No critical finding
  and no way for an unprivileged actor to move funds out of the policy
  wallet, bypass its caps, or take owner authority on chain. The 3 high,
  7 medium and 22 low or informational findings were in the paid API, the
  hosted demo, the engine, the SDK, CI and the docs. Every one is fixed or
  documented, each with a regression test:
  [docs/security/redteam-2026-11-13.md](docs/security/redteam-2026-11-13.md).
- **Earlier internal reviews:** [docs/security-review.md](docs/security-review.md).
- **Threat model and accepted residual risks:**
  [docs/threat-model.md](docs/threat-model.md).

The contracts have not had a professional audit.

## Security posture (honest statement)

- The policy wallet and the wallet factory are deployed on NEAR mainnet
  with no access keys and no upgrade path. A contract fix ships as a new
  factory and new wallets, never as an upgrade. They are unaudited beta
  contracts: use small amounts. A wallet accepts at most 100 USDC and
  10 wNEAR through `ft_transfer_call`; a plain `ft_transfer` is not checked.
- The agent never holds a key that can move tokens: its function-call key
  can call only `pay` on its wallet, and every `pay` is checked on chain
  against the payee allowlist, the per-call cap and the per-day cap.
- The paid HTTP API is a demonstrator. Its ledger is in memory and
  process-local, it binds loopback, and one development caller acts for
  every request; there is no real authentication boundary yet. Jobs and
  accounts are bound to the caller that created them, so the boundary is in
  place for when authentication lands.
- The NEAR payment adapter is verification-only: it reads the chain over
  JSON-RPC and holds no keys. A payment counts only when the token
  contract's own NEP-141 event, in a finalized receipt, pays the gateway
  with the server-issued memo.
- The mainnet scripts in `scripts/mainnet/` sign transactions from key
  files outside the repository that the operator names on the command
  line. The repository contains no keys.
- The hosted demo holds a testnet owner key and testnet and mainnet
  pay-only agent keys as server environment variables. The public page can
  approve only a held payment to the allowlisted payee, cannot apply a
  policy, and accepts only same-origin JSON requests. The agent key stops
  signing at an on-chain gas-allowance floor.
- The scanner runs as a subprocess with a hard timeout; its input is
  treated as hostile data and never interpreted.

## Non-goals and guarantees

- Request bodies are never logged.
- Client assertions are never accepted as proof of payment; credit is
  funded only from provider-observed confirmations.
- URLs found in evidence fields are never fetched.
- No floating-point value is used anywhere near money; balances are
  integer atomic units.

