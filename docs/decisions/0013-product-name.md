# ADR 0013: Product name

Status: accepted

Date: 2026-11-10

## Context

The working name "AgentPay Sentinel" is used by several unrelated public repositories, at least
one of which (created 2026-11-09) also describes security tooling for x402 agent payments. A
reader, human or AI, could not tell the projects apart, and the hackathon requires a unique
project name.

## Decision

The product is called **TriagePay**: an AI security agent pays per call, through x402 on NEAR and
under an on-chain spend policy, for a deterministic triage verdict on a security finding.

Internal identifiers keep the `aps` prefix from the working name: the Rust crates (`aps-core`,
`aps-api`, ...), the Python packages (`aps_scanner`, `aps_client`) and the `APS_*` environment
variables. Renaming several hundred identifiers days before the build freeze would add risk and
change no behaviour.

## Consequences

- Prose, package descriptions, UI titles, container image names and the licence holder read
  "TriagePay".
- Earlier commit messages keep the old name; nothing is rewritten for it.
