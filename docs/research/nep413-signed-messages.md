# NEP-413 signed messages (research, 2026-10-21)

Scope: prompt 08 (NEAR-native signed requests). Primary source: NEP-413 "Near Wallet API -
support for signMessage method", Status **Final**,
https://github.com/near/NEPs/blob/master/neps/nep-0413.md (accessed 2026-10-21). Status legend as
in `near-contract-toolchain.md`.

## 1. Input and payload

| Claim | Status |
|---|---|
| Input `SignMessageParams`: `message: string`, `recipient: string` (for example `"myapp.com"`), `nonce: [u8; 32]`, optional `callbackUrl?: string`, optional `state?: string` (browser wallets, CSRF state). | Verified-primary |
| Signed `Payload` struct, in this field order: `message: string`, `nonce: [u8; 32]`, `recipient: string`, `callbackUrl?: string`. `state` is **not** part of the signed payload. | Verified-primary |
| Design rules: signed off-chain; must include recipient and nonce; "Cannot represent a valid transaction"; "Must be signed using a Full Access Key". | Verified-primary |
| Output `SignedMessage`: `accountId`, `publicKey` (`"<key-type>:<base58>"`, for example `ed25519:...`), `signature` (**base64**), optional `state`. Web wallets return these as URL **fragments** (`#accountId=...&publicKey=...&signature=...`), not query parameters. | Verified-primary |

## 2. Bytes that are signed

1. Borsh-serialize the tag `u32` 2^31 + 413 = **2147484061**, little-endian bytes
   **`9d 01 00 80`**. The tag makes the bytes an invalid transaction, because a transaction
   begins with a borsh `signerId` string length and this length would be far too long.
2. Append `borsh(Payload)`:
   - `message`: u32 little-endian length followed by UTF-8 bytes;
   - `nonce`: 32 raw bytes (fixed array, no length prefix);
   - `recipient`: u32 length followed by UTF-8 bytes;
   - `callbackUrl`: borsh `Option<String>`, one byte `0x00` (none) or `0x01` followed by the string.
3. `hash = SHA-256(tag_bytes || payload_bytes)`.
4. The wallet signs `hash` with an ed25519 full-access key. NEP-413 notes that NEAR signatures are
   "Ed25519 signatures of a SHA-256 hash", the same convention as transactions.

Status: steps 1-4 are Verified-primary (NEP text). The byte-level borsh encoding of `String`,
`[u8; 32]` and `Option` follows the borsh specification (https://borsh.io). Some docs examples
model the tag as a leading `u32` field of the struct; that produces the same bytes.

## 3. Server-side verification steps (for prompt 08)

Derived from NEP-413 plus the RPC behaviour recorded in `rpc-endpoints.md`. Status:
Recommendation; each step maps to a NEP rule or an RPC fact.

1. Parse `accountId`, `publicKey`, `signature` (base64, 64 bytes for ed25519). Reject key types
   that are not supported (NEAR also has secp256k1 and, since nearcore 2.13, ML-DSA-65 keys).
2. Rebuild `Payload` **from server-side values**: `recipient` = the service's fixed identifier
   (never taken from the client); `nonce` = a server-issued or server-validated 32-byte value;
   `message` = canonical text of `{method, path, body_sha256, timestamp}` in a fixed
   serialization; `callbackUrl` = none for API clients.
3. Compute the hash as in section 2 and verify the ed25519 signature against `publicKey`.
4. Check key ownership and permission with a read-only RPC `query`, `request_type:
   "view_access_key"`, `account_id`, `public_key`, `finality: "final"`. Require
   `permission == "FullAccess"`; NEP-413 requires a full-access key, and a function-call key is
   something "any third-party can ask the user to create". Prefer `view_access_key` (one key)
   over `view_access_key_list`.
5. Replay protection: store the nonce (or `accountId + nonce`) until the timestamp window
   expires and reject reuse; reject timestamps outside a short window.
6. Cache the key lookup briefly, and re-query on failure. A deleted key must stop working within
   the cache TTL.

## 4. RPC details relevant to step 4

| Claim | Source | Accessed | Status |
|---|---|---|---|
| `view_access_key_list` returns `{block_hash, block_height, keys: [{public_key, access_key: {nonce, permission}}]}` with `permission` either the string `"FullAccess"` or an object `{"FunctionCall": {allowance, receiver_id, method_names}}`. | live read-only call on mainnet (USDC contract account); https://docs.near.org/protocol/access-keys | 2026-10-21 | Verified-primary |
| nearcore 2.14 (testnet now, not yet mainnet) adds pagination to `view_access_key_list` (`after_key`, `limit`, `last_key`), caps responses at 100 keys by default, and makes an unpaginated list fail for accounts with more than 100 keys. It also returns `UNKNOWN_ACCOUNT` (instead of a missing-key error or an empty list) for a non-existent account. | https://raw.githubusercontent.com/near/nearcore/master/CHANGELOG.md (2.14.0) | 2026-10-21 | Verified-primary |

## Implications for the build

- **Prompt 08:** the plan's `{method, path, body_hash, nonce, timestamp}` fits inside NEP-413 as
  the `message` string. `recipient` must be a fixed service identifier; `nonce` is the 32-byte
  NEP-413 nonce; `state` is not signed and must not carry security data.
- **Prompt 08:** check `FullAccess` explicitly. "The key is on the account" is not enough, since
  a FunctionCall key would otherwise authenticate as the account.
- **Prompt 08:** use `view_access_key` per key to avoid the 2.14 pagination limit, and handle
  both the old flat missing-key response and the new `UNKNOWN_ACCOUNT` error.
- **Prompt 08:** a policy-wallet agent account that holds only a function-call key cannot produce
  a valid NEP-413 signature. Agents authenticate with API keys, or with a separate full-access
  identity key.
- **Prompt 08:** tests: the vector from the NEP example (message `"hi"`, nonce `[0..31]`,
  recipient `"myapp.com"`, callbackUrl `"myapp.com/callback"`) as a serialization fixture; a
  wrong recipient; a reused nonce; a function-call key; a deleted key.
