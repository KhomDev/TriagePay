"""Offline x402 walkthrough against a local API with the mock provider.

Start the API in one terminal (no keys, no chain):

    cargo run -p aps-api --bin aps-api

Then, in another:

    python scripts/x402-demo.py [--api http://127.0.0.1:8080]

It shows the three protocol steps: the 402 with its payment requirement,
the retry with a payment proof, and the replay of the same proof. With the
mock provider the network is the development-only ``aps:mock`` and any
transaction hash is accepted, so this demonstrates the HTTP exchange and the
ledger, not payment verification. The run on NEAR testnet, where the server
verifies the token contract's receipt, is docs/demo/transcript-2026-11-11.md.

Standard library only.
"""

import argparse
import base64
import json
import pathlib
import sys
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent


def post(url, body, headers=None):
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={"content-type": "application/json", **(headers or {})},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, dict(response.headers), json.load(response)
    except urllib.error.HTTPError as err:
        return err.code, dict(err.headers), json.load(err)


def header(headers, name):
    for key, value in headers.items():
        if key.lower() == name:
            return json.loads(base64.b64decode(value))
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--api", default="http://127.0.0.1:8080")
    parser.add_argument("--envelope", default=str(ROOT / "demo" / "envelope.json"))
    options = parser.parse_args()
    api = options.api.rstrip("/")
    envelope = json.loads(pathlib.Path(options.envelope).read_text(encoding="utf-8"))

    print("1. POST /v2/scan with no payment")
    status, headers, _ = post(api + "/v2/scan", envelope)
    required = header(headers, "payment-required")
    if status != 402 or required is None:
        print(f"   expected HTTP 402 with PAYMENT-REQUIRED, got {status}")
        return 1
    requirement = required["accepts"][0]
    print(f"   HTTP {status}; PAYMENT-REQUIRED decodes to:")
    print("   " + json.dumps(requirement, indent=2).replace("\n", "\n   "))
    if requirement["network"] != "aps:mock":
        print("   this server settles on a real network; this script only drives the mock")
        return 1

    print("2. Retry with PAYMENT-SIGNATURE (the mock accepts any transaction hash)")
    payload = {
        "x402Version": 2,
        "resource": {"url": "/v2/scan"},
        "accepted": requirement,
        "payload": {
            "network": requirement["network"],
            "intent_id": requirement["extra"]["memo"],
            # One made-up hash per requirement: a hash is single-use.
            "tx_hash": "mock-tx-" + requirement["extra"]["memo"],
            "signer_id": "mock-payer",
        },
    }
    signature = base64.b64encode(json.dumps(payload).encode("utf-8")).decode("ascii")
    status, headers, body = post(api + "/v2/scan", envelope, {"PAYMENT-SIGNATURE": signature})
    if status != 200:
        print(f"   expected HTTP 200, got {status}: {json.dumps(body)[:300]}")
        return 1
    report = body["report"]
    print(f"   HTTP {status}; verdict: {report['decision']}")
    print(f"   missing evidence: {report.get('missing_evidence')}")
    print(f"   content hash the payment settled against: {report['content_hash']}")
    print(f"   charged: {body['settlement']['charged']['decimal']}")
    print("   PAYMENT-RESPONSE: " + json.dumps(header(headers, "payment-response")))

    print("3. Send the same proof and request again")
    status, _, again = post(api + "/v2/scan", envelope, {"PAYMENT-SIGNATURE": signature})
    same = status == 200 and again == body
    print(f"   HTTP {status}; identical response, no second charge: {same}")

    print("4. Reuse the proof for a different finding")
    other = dict(envelope, title="A different finding")
    status, headers, body = post(api + "/v2/scan", other, {"PAYMENT-SIGNATURE": signature})
    print(f"   HTTP {status}; reason: {body.get('error', {}).get('code')}")
    return 0 if same and status == 402 else 1


if __name__ == "__main__":
    sys.exit(main())
