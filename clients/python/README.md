# aps_client (Python, standard library only)

An x402 v2 client for the AgentPay Sentinel `/v2/scan` resource, using the
near-tx-proof method (scheme `exact`, `paymentFlow: "upfront"`). The
protocol profile is in [`docs/protocols/x402-near.md`](../../docs/protocols/x402-near.md).

The library holds no key material. Paying is a pluggable callable that
receives the selected payment requirement and returns
`{"tx_hash": ..., "signer_id": ...}`.

## Pay for a tool call in 10 lines

```python
import sys

sys.path.insert(0, "clients/python")
from aps_client import Client, shell_payer

# The TypeScript reference payer signs with a key file kept outside the repository.
payer = shell_payer(
    [
        "node",
        "clients/typescript/src/cli.ts",
        "pay",
        "--mode",
        "direct",
        "--account",
        "agent.testnet",
        "--key-file",
        "/path/outside/repo/agent.json",
    ]
)
result = Client("http://127.0.0.1:8080").pay_and_scan({"title": "...", "...": "..."}, pay=payer)
print(result.report["decision"], result.payment_response["transaction"])
```

What `pay_and_scan` does:

1. `POST /v2/scan` without payment and decode the `PAYMENT-REQUIRED` header.
2. Select the `accepts[]` entry with `assetTransferMethod: "near-tx-proof"`
   and `paymentFlow: "upfront"` (entries with an unknown flow are skipped).
3. Call `pay(requirement)`. The payer must transfer exactly
   `requirement["amount"]` of `requirement["asset"]` to
   `requirement["payTo"]` with memo `requirement["extra"]["memo"]`, using
   `ft_transfer` directly or a policy wallet's `pay` (never
   `ft_transfer_call`).
4. Retry with `PAYMENT-SIGNATURE` (a full v2 `PaymentPayload` whose
   `payload` is `{network, intent_id, tx_hash, signer_id}`). While the
   transaction is not final the server answers `402 settlement_pending`
   and the client retries the same proof, honoring `Retry-After`.
5. Return the report and the decoded `PAYMENT-RESPONSE`.

Failures raise `X402Error` with `status`, `reason` (the `errorReason`) and
the decoded `payment_response`.

## Tests

The tests run against an in-process mock server (offline):

```sh
python -m pytest tests/test_python_client.py -q
```
