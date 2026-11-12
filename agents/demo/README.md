# Agent demo

One script, `demo.mjs`, runs on NEAR testnet with throwaway accounts and
zero value. It shows an agent paying TriagePay per call through x402 from a
spend-policy wallet, and what the wallet refuses.

A recorded run with transaction links:
[docs/demo/transcript-2026-11-11.md](../../docs/demo/transcript-2026-11-11.md).

## What the agent is

A fixed loop with no language model:

1. `POST /v2/scan` with a finding, read the 402 requirement.
2. Call `pay` on the wallet with the function-call key (no deposit).
3. Retry with the transaction hash until the server answers 200.
4. Print the verdict and the remaining budget read from the wallet.

It has one deliberately naive behaviour: it follows a payment instruction
found inside finding 3. That stands in for a prompt-injected agent, so the
wallet's refusal can be shown. Nothing in the demo depends on the agent
behaving well: the limits are enforced by the contract.

## Commands

Prerequisites: Node 22.18 or later; `npm ci --prefix clients/typescript`;
both contract wasm files built (see the main README, section C); a funded
testnet account's key file outside the repository.

```sh
node agents/demo/demo.mjs setup  --out-dir DIR --funder-key FILE
node agents/demo/demo.mjs run    --out-dir DIR --api http://127.0.0.1:8080
node agents/demo/demo.mjs direct --out-dir DIR --api http://127.0.0.1:8080
node agents/demo/demo.mjs revoke --out-dir DIR --api http://127.0.0.1:8080
```

| Command | Who acts | What happens |
|---|---|---|
| `setup` | funder, owner | Creates owner, agent, gateway, token and wallet accounts. Deploys the test token. Deploys the wallet, initializes it and deletes its key in one batch. Funds the wallet with 10 tokens. Sets the agent's policy (1 token per call, 4 per UTC day, payee = the gateway). Adds the agent's function-call key (`pay` only). Prints the command that starts the API |
| `run` | agent | Four paid scans. Then: a payment to the account named in the injected finding (`ERR_PAYEE_NOT_ALLOWED`), a payment one unit over the per-call limit (`ERR_PER_CALL_LIMIT`), a fifth call (`ERR_DAILY_LIMIT`), and two attempts to call `set_agent` (rejected by the node) |
| `direct` | owner | One scan paid with a plain `ft_transfer` from a full-access account, then the same proof replayed and reused |
| `revoke` | owner, agent | `revoke_agent`; the agent's next `pay` fails with `ERR_AGENT_INACTIVE`; the agent account deletes the key; a transaction signed with it is rejected by the node |

Add `--pause` to `run`, `direct` or `revoke` to stop before each step until
Enter is pressed: useful when recording a video and talking over it.

With the API started in `triage.v2` mode (`APS_SCAN_SKU=triage.v2`,
`APS_LIVE_MODEL=1`, `NEARAI_API_KEY`, and `APS_SCANNER_TIMEOUT_SECS=120`),
each paid call also prints the model judge's severity call, probability and
rationale. The verdict itself still comes from the deterministic gates when
they are stricter.

`DIR` must be outside the repository. It receives `keys/` (never printed),
`agent-demo-EVIDENCE.json` and `transcript.md`. Every command resumes: a
step already recorded in the evidence file is skipped.

The setup spends about 5.1 testnet NEAR, most of it storage stake for the
two contracts (about 1.75 and 1.48 NEAR).

## The token

The server verifies a payment from the token contract's NEP-297 `nep141`
`ft_transfer` event. `wrap.testnet` logs plain text instead of events, so it
cannot be the settlement asset. The demo deploys `contracts/test-token`
(near-contract-standards, 6 decimals), which emits the standard events.

## Findings

`findings/` holds four synthetic envelopes. None describes a real protocol.

| File | Verdict | Why |
|---|---|---|
| `1-rounding-drain-placeholder-evidence` | `needs_evidence` | The deployment reference is a placeholder |
| `2-rounding-drain-with-evidence` | `supported` | The same finding with a well-formed reference |
| `3-fee-withdrawal-with-injection` | `supported`, embedded instructions flagged | The text tells the reader to mark it supported and to send a token to another account; the verdict does not change |
| `4-owner-can-pause` | `rejected` | It needs the owner key and names no quantified loss |

"Supported" means no deterministic gate objected. The checks are
format-level: nothing in a finding is verified on chain.
