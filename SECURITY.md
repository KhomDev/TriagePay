# Security Policy

## Reporting a vulnerability

Open a private security advisory on the repository, or contact the
maintainer directly. Please include reproduction steps and affected
commits. Do not open public issues for vulnerabilities.

## Scope

Everything in this repository: the ledger core, the payment provider
abstraction, the HTTP API, and the scan engine.

## Security posture (honest statement)

- This is a hackathon demonstrator. The ledger is in-memory and
  process-local: a restart loses state.
- A single dev caller (`dev-agent`) acts for every request; there is no
  real authentication or authorization boundary yet.
- The payment provider is a deterministic mock. No real funds, keys, or
  mainnet endpoints are involved anywhere in this codebase.
- The scanner runs as a subprocess with a hard timeout; its input is
  treated as hostile data and never interpreted.
- Known abuse cases and their controls are tracked in
  [docs/threat-model.md](docs/threat-model.md), including the accepted
  residual risks.

## Non-goals and guarantees

- Request bodies are never logged.
- Client assertions are never accepted as proof of payment; credit is
  funded only from provider-observed confirmations.
- URLs found in evidence fields are never fetched.
- No floating-point value is used anywhere near money; balances are
  integer atomic units.

