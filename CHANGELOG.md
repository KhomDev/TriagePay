# Changelog

All notable changes to this project are documented here. The format
follows Keep a Changelog; the project has not shipped a tagged release.

## [Unreleased]

### Added
- Prepaid ledger core: integer money, idempotent deposits, reservations,
  settlements, refunds, conservation fuzz, SKU price book.
- Payment provider trait with pure reconciliation and a deterministic
  mock provider (finality delay, mismatched payments, expiry, one-shot
  refunds).
- HTTP API: accounts, deposits, reservations, job lifecycle, scan
  execution, payment intents, single error shape, strict bodies.
- Python scan engine: envelope schema, hostile-input normalization with
  injection flagging, nine deterministic gates, decision matrix,
  content-hashed public reports, CLI bridge.
- End-to-end scan flow over the real subprocess: payment covers the scan,
  not the verdict; invalid envelopes never charge.
- Demo scripts (bash + PowerShell) walking funding through refund.
- Architecture, threat model, ADRs 0001-0003, CI pipeline, security and
  contribution guides.

### Security
- Credit funds only from provider-observed confirmations matching amount
  and asset; client assertions have no funding path.
- Settlement requires the Running state and a non-zero 32-byte report
  hash; refunds are one-shot; replays never move funds twice.

