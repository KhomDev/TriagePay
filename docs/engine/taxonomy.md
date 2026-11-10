# Public triage taxonomy

This list is the public contract of the triage engine v2. A report names a finding's problems only
with these category ids. Every gate in a v2 ruleset maps onto exactly one of them: the public
ruleset `rules-v1.public` and the off-repo private ruleset alike. The Stage B model may only return
ids from this list; any other id is a schema violation and fails closed.

The ids are generic on purpose. They describe why a finding is unlikely to be paid at medium or
higher severity, never which private pattern noticed it.

Source of truth in code: `scanner/aps_scanner/taxonomy.py` (mirrored in
`knowledge-compiler/knowledge_compiler/schema.py`; a test keeps the two equal).

| Category id | Definition | Stage A outcome in `rules-v1.public` | Synthetic example that hits it |
|---|---|---|---|
| `privileged-actor-required` | The attack needs an admin, owner, governance, keeper or other privileged actor, and the finding names no permissionless route. | Fail (rejected) | Attacker capability: "The protocol owner". |
| `no-quantified-impact` | No amount, percentage or token quantity is bound to a loss verb. | NeedsEvidence | "Later depositors lose their deposits." |
| `self-harm-only` | Only the caller's own position is harmed, or the loss needs user error. | Fail (rejected) | "Only affects the caller who passes a zero slippage bound by mistake." |
| `theoretical-only` | The impact is hedged ("in theory", "could potentially") and no reproduction or code context is supplied; or no reproduction material is supplied at all. | NeedsEvidence | "In theory a victim could lose 10,000 USDC." with no reproduction. |
| `design-intent-likely` | The finding itself, or the supplied program rules, describe the behaviour as intended or documented design. | Held | "Rounding toward the vault is by design and documented." |
| `out-of-scope-component` | The component matches an exclusion section of the supplied `program_rules`, or the scope statement places it out of scope. | Fail (rejected) | Program rules list "vault contracts" under "Out of scope". |
| `known-issue-declared` | A known-issue declaration is attached, the program rules list a matching known issue, or the finding calls itself previously reported. | Held | `known_issue_declaration`: "Possible duplicate of a listed issue". |
| `concession-contradicts-impact` | The finding concedes no loss, no attacker profit or fee-only drag while it claims value impact. | Fail (rejected) | "Note that no funds are lost in this flow." with a loss claim. |
| `unverified-deployment-evidence` | The deployment reference is absent, malformed or a placeholder, or cites no well-formed on-chain reference. Checks are format-level only: nothing is verified on chain. | NeedsEvidence | "Vault live at 0x1234567890abcdef" (16 hex digits: neither an address nor a hash). |

## How the categories are detected (Stage A)

The public ruleset uses structured evaluators, not bare substring tests:

- **Negation-aware phrase matching.** A phrase counts only when no negator ("no", "not", "never",
  "without", "cannot", any "n't" contraction) appears within four tokens before it in the same
  clause. Clauses end at sentence punctuation and at "but", "however", "although" and similar
  words. "No admin role is needed" therefore does not count as privileged.
- **Precondition-privilege classification.** Bare role words are read from
  `attacker_capability`; requirement phrases ("requires the owner", "malicious admin") are read
  from `preconditions`, `hypothesis` and `mechanism`. A permissionless escape ("anyone",
  "permissionless", "any external") cancels the hit unless it is itself negated.
- **Quantification detection.** An amount (`$250k`), a percentage (`35%`, `bps`), a token quantity
  (`1,200 ETH`, `1e24 tokens`) or an absolute phrase ("all deposits") must sit within twelve
  tokens of an affirmed impact verb in the same sentence.
- **Concession detection.** A concession phrase counts only in a sentence without a rebuttal
  marker: "The sponsor claims no funds are lost, but the PoC drains 10 ETH" is not a concession.
- **Scope cross-check.** When `program_rules` is supplied, sections introduced by exclusion,
  known-issue or design-intent markers are compared with the component class or the title.
- **Evidence-reference format checks.** A `0x` hex run must be 40 (address) or 64 (hash) digits
  and must not be an obvious fabrication (a counting run, a repeated unit, three or fewer distinct
  digits); placeholder markers such as `0x...` or `<address>` count as placeholders. A pinned
  `source_ref` (an https repository URL and a 7-40 digit commit) stands in for deployment evidence
  on pre-deployment reviews.

Each non-passing branch carries a penalty factor that feeds the benchmark score. The factors in
`rules-v1.public` are untuned placeholders; the benchmark evaluation (prompt 05) owns their
values. No accuracy figure is claimed for any category.

## Private ruleset mapping

The private ruleset is compiled outside the repository. Its gates get opaque ids, and each one
names one of the categories above, so a report produced with it carries the same public vocabulary
and nothing else. Only its manifest hash is ever printed or documented.
