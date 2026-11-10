# Authentication

Two methods identify a caller. Both produce the caller id that scopes idempotency keys. The
primitives live in `crates/auth` (`aps-auth`).

Wiring status: the primitives are implemented and tested. The HTTP middleware that reads the
headers below is planned for wave 2 (runbook, "Wave-2 wiring"). Until then the API acts for the
single development caller.

## 1. API keys

```
Authorization: Bearer aps_<64 lowercase hex characters>
```

- The key is 32 random bytes from the operating system CSPRNG. The token is `aps_` followed by
  their hex encoding; the prefix makes a leaked key easy to search for.
- The server stores only SHA-256 of the 32 bytes and finds the caller by that digest.
- Unknown and revoked keys get the same `401` (`unauthenticated`).
- The operator manages keys with `aps-admin create-caller`, `rotate-caller`, `revoke-caller` and
  `list-callers` (see the runbook).

API keys are the method for agents that hold only a **function-call** key, such as an agent whose
NEAR account is limited to its policy wallet's `pay` method. NEP-413 cannot serve those agents
(see the next section).

## 2. NEP-413 signed requests

"Your NEAR account is your API identity": the caller signs each request with a full-access key
of their NEAR account, and the server checks that key against the chain. The caller id becomes
`near:<account_id>`.

Source: NEP-413 (Status Final), https://github.com/near/NEPs/blob/master/neps/nep-0413.md,
accessed 2026-10-21. Research notes: `docs/research/nep413-signed-messages.md`.

**A full-access key is required.** NEP-413 messages must be signed with a full-access key. A
function-call key is something any third-party application can ask a user to add, so it proves
nothing about account control. The server checks the key's permission and rejects
`FunctionCall` keys with `403 full_access_key_required`. Agents that hold only function-call keys
use API keys instead.

### Headers

| Header | Value |
|---|---|
| `X-NEAR-Account` | the NEAR account id, for example `alice.testnet` |
| `X-NEAR-Public-Key` | `ed25519:<base58 public key>`; other key types are rejected |
| `X-NEAR-Signature` | standard base64 of the 64-byte ed25519 signature |
| `X-NEAR-Nonce` | standard base64 of 32 random bytes, new for every request |
| `X-NEAR-Timestamp` | unix time in milliseconds |

### What is signed

1. **message**: this exact JSON, with keys in this order and no whitespace:

   ```
   {"scheme":"aps-nep413-request-v1","method":"POST","path":"/reservations?x=1","body_sha256":"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a","timestamp_ms":42}
   ```

   - `method` is the uppercase HTTP method.
   - `path` is the path plus query exactly as sent. It must be printable ASCII without `"` or
     `\`.
   - `body_sha256` is the lowercase hex SHA-256 of the raw request body (the example is the
     body `{}`).
   - `timestamp_ms` equals `X-NEAR-Timestamp`.

   In JavaScript, `JSON.stringify({scheme, method, path, body_sha256, timestamp_ms})` produces
   the same bytes.
2. **payload bytes**: the borsh u32 tag 2^31 + 413, little-endian `9d 01 00 80`, followed by the
   borsh payload:
   - `message`: u32 little-endian length, then the UTF-8 bytes;
   - `nonce`: the 32 raw bytes;
   - `recipient`: u32 length, then the bytes. It is the service identifier configured on the
     server (`APS_NEP413_RECIPIENT`, planned) and never taken from the client;
   - `callbackUrl`: a single `00` byte (none).
3. **signature**: ed25519 over `SHA-256(payload bytes)`, the convention NEAR wallets use for
   NEP-413 `signMessage`.

`SignedRequest::sign` in `crates/auth/src/nep413.rs` is a reference client implementation. A
NEP-413-compliant wallet's `signMessage({message, recipient, nonce})` signs the same bytes. That
is untested here against a real wallet; wallet interoperability is pending a testnet run.

### Server checks, in order

1. The account id is a valid NEAR account id; the key type is ed25519; the signature and nonce
   decode to 64 and 32 bytes.
2. The timestamp lies between 5 minutes in the past and 30 seconds in the future
   (`401 stale_request`).
3. The server rebuilds the message from its own view of the method, the path and the body, and
   rebuilds the payload with its own recipient. The signature must verify under
   `verify_strict` (`401 bad_signature`).
4. The key belongs to the account and has `FullAccess` permission. This is read-only RPC: a
   `query` with `request_type: "view_access_key"` at `finality: "final"`. When only
   `view_access_key_list` is available, it is read page by page (nearcore 2.14 `after_key` and
   `limit`, capped at a fixed number of pages). Positive results are cached briefly, so a key
   deleted on chain stops working within the cache TTL.
5. The nonce is unused for this account (`401 nonce_replayed`). It is recorded in SQLite until
   its timestamp leaves the window, so a restart does not reopen a replay window. The nonce is
   consumed only after checks 1-4 pass, so a forged request cannot burn a legitimate nonce.

The `state` field from NEP-413 browser flows is not signed, and the server ignores it.

### Errors

| Status | `code` | Meaning |
|---|---|---|
| 400 | `malformed_auth`, `unsupported_key_type` | a header is missing or malformed |
| 401 | `unauthenticated` | API key unknown, revoked or malformed |
| 401 | `stale_request` | timestamp outside the window |
| 401 | `bad_signature` | signature does not verify over the rebuilt payload |
| 401 | `key_not_on_account` | key not on the account, or the account does not exist |
| 401 | `nonce_replayed` | nonce already used |
| 403 | `full_access_key_required` | the key is a function-call key |
| 503 | `key_lookup_unavailable` | RPC unavailable, or too many key pages |

No error message echoes a key, a signature or a nonce.
