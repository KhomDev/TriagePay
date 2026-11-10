"""Python x402 client tests against a mock-provider server (offline).

The mock server speaks the same /v2/scan protocol as crates/api/src/x402.rs:
402 with PAYMENT-REQUIRED, then PAYMENT-SIGNATURE, settlement_pending
while the "chain" is not final, 200 with PAYMENT-RESPONSE.

Run from the repository root: python -m pytest tests -q
"""

import json
import pathlib
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "clients" / "python"))

from aps_client import (
    Client,
    X402Error,
    build_payment_payload,
    decode_header,
    encode_header,
    select_requirement,
    shell_payer,
)

NETWORK = "near:testnet"
ASSET = "usdc.mock.testnet"
GATEWAY = "gateway.mock.testnet"
PRICE = "1000000"


class MockSentinel:
    """State of the mock server: issued memos, pending polls, payments."""

    def __init__(self, pending_polls=1, settle_reason=None):
        self.pending_polls = pending_polls
        self.settle_reason = settle_reason
        self.issued = {}
        self.polls = {}
        self.settled = {}
        self.charges = 0
        self.lock = threading.Lock()

    def requirement(self, memo):
        return {
            "scheme": "exact",
            "network": NETWORK,
            "amount": PRICE,
            "asset": ASSET,
            "payTo": GATEWAY,
            "maxTimeoutSeconds": 900,
            "extra": {
                "assetTransferMethod": "near-tx-proof",
                "paymentFlow": "upfront",
                "memo": memo,
                "intentId": memo,
            },
        }


def make_handler(mock):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, status, body, headers=None):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            for key, value in (headers or {}).items():
                self.send_header(key, value)
            self.end_headers()
            self.wfile.write(data)

        def fail(self, status, reason, tx=""):
            response = {
                "success": False,
                "errorReason": reason,
                "transaction": tx,
                "network": NETWORK,
            }
            headers = {"PAYMENT-RESPONSE": encode_header(response)}
            if reason == "settlement_pending":
                headers["Retry-After"] = "0"
            self.reply(status, {"error": {"code": reason, "message": reason}}, headers)

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            envelope = json.loads(self.rfile.read(length) or b"{}")
            signature = self.headers.get("PAYMENT-SIGNATURE")
            with mock.lock:
                if signature is None:
                    memo = f"aps2_mock_{len(mock.issued) + 1:05d}"
                    mock.issued[memo] = mock.requirement(memo)
                    required = {
                        "x402Version": 2,
                        "error": "PAYMENT-SIGNATURE header is required",
                        "resource": {"url": "/v2/scan", "mimeType": "application/json"},
                        "accepts": [mock.issued[memo]],
                    }
                    self.reply(
                        402,
                        {"error": {"code": "payment_required", "message": "pay"}},
                        {"PAYMENT-REQUIRED": encode_header(required)},
                    )
                    return
                payload = decode_header(signature)
                proof = payload["payload"]
                memo = proof["intent_id"]
                if payload.get("x402Version") != 2 or memo not in mock.issued:
                    self.fail(400, "invalid_payload")
                    return
                if payload["accepted"] != mock.issued[memo]:
                    self.fail(402, "invalid_payment_requirements", proof["tx_hash"])
                    return
                if mock.settle_reason:
                    self.fail(402, mock.settle_reason, proof["tx_hash"])
                    return
                polls = mock.polls.get(memo, 0)
                if polls < mock.pending_polls:
                    mock.polls[memo] = polls + 1
                    self.fail(402, "settlement_pending", proof["tx_hash"])
                    return
                if memo not in mock.settled:
                    mock.charges += 1
                    mock.settled[memo] = {
                        "report": {"decision": "supported", "title": envelope.get("title")},
                        "payment": {"intent_id": memo, "transaction": proof["tx_hash"]},
                    }
                response = {
                    "success": True,
                    "transaction": proof["tx_hash"],
                    "network": NETWORK,
                    "payer": proof["signer_id"],
                    "amount": PRICE,
                }
                self.reply(200, mock.settled[memo], {"PAYMENT-RESPONSE": encode_header(response)})

    return Handler


@pytest.fixture
def server():
    started = []

    def start(**kwargs):
        mock = MockSentinel(**kwargs)
        httpd = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(mock))
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        started.append(httpd)
        return f"http://127.0.0.1:{httpd.server_address[1]}", mock

    yield start
    for httpd in started:
        httpd.shutdown()
        httpd.server_close()


def recording_payer(calls):
    def pay(requirement):
        calls.append(requirement)
        return {"tx_hash": f"tx-{requirement['extra']['memo']}", "signer_id": "agent.mock.testnet"}

    return pay


def test_one_paid_scan_end_to_end(server):
    url, mock = server(pending_polls=2)
    calls = []
    client = Client(url, sleep=lambda _seconds: None)
    result = client.pay_and_scan({"title": "finding"}, pay=recording_payer(calls))
    assert len(calls) == 1
    requirement = calls[0]
    assert requirement["payTo"] == GATEWAY
    assert requirement["amount"] == PRICE
    assert requirement["extra"]["memo"].startswith("aps2_mock_")
    assert result.report["decision"] == "supported"
    assert result.payment_response["success"] is True
    assert result.payment_response["transaction"] == f"tx-{requirement['extra']['memo']}"
    assert result.attempts == 3  # two settlement_pending answers, then 200
    assert mock.charges == 1


def test_settlement_pending_retries_are_bounded(server):
    url, _mock = server(pending_polls=10)
    client = Client(url, sleep=lambda _seconds: None)
    with pytest.raises(X402Error) as caught:
        client.pay_and_scan({"title": "x"}, pay=recording_payer([]), max_pending_retries=2)
    assert caught.value.reason == "settlement_pending"
    assert caught.value.payment_response["transaction"].startswith("tx-")


def test_typed_failures_surface_the_reason(server):
    url, _mock = server(settle_reason="amount_mismatch")
    client = Client(url, sleep=lambda _seconds: None)
    with pytest.raises(X402Error) as caught:
        client.pay_and_scan({"title": "x"}, pay=recording_payer([]))
    assert caught.value.status == 402
    assert caught.value.reason == "amount_mismatch"


def test_network_filter_and_selection():
    required = {
        "x402Version": 2,
        "accepts": [
            {"scheme": "exact", "network": "near:mainnet", "extra": {}},
            {
                "scheme": "exact",
                "network": "near:testnet",
                "extra": {
                    "assetTransferMethod": "near-tx-proof",
                    "paymentFlow": "escrow",
                    "memo": "m",
                },
            },
            {
                "scheme": "exact",
                "network": "near:testnet",
                "extra": {
                    "assetTransferMethod": "near-tx-proof",
                    "paymentFlow": "upfront",
                    "memo": "m2",
                },
            },
        ],
    }
    assert select_requirement(required)["extra"]["memo"] == "m2"
    with pytest.raises(X402Error):
        select_requirement(required, network="near:mainnet")
    with pytest.raises(X402Error):
        select_requirement({"x402Version": 1, "accepts": []})


def test_payload_shape_and_header_round_trip():
    requirement = {
        "scheme": "exact",
        "network": NETWORK,
        "amount": PRICE,
        "asset": ASSET,
        "payTo": GATEWAY,
        "maxTimeoutSeconds": 900,
        "extra": {
            "assetTransferMethod": "near-tx-proof",
            "paymentFlow": "upfront",
            "memo": "aps2_x",
        },
    }
    payload = build_payment_payload(
        requirement, {"tx_hash": "h", "signer_id": "s"}, {"url": "/v2/scan"}
    )
    assert payload["x402Version"] == 2
    assert payload["accepted"] == requirement
    assert payload["payload"] == {
        "network": NETWORK,
        "intent_id": "aps2_x",
        "tx_hash": "h",
        "signer_id": "s",
    }
    assert decode_header(encode_header(payload)) == payload
    with pytest.raises(X402Error):
        build_payment_payload(requirement, {"tx_hash": "", "signer_id": "s"})


def test_shell_payer_runs_an_external_command(server):
    url, mock = server(pending_polls=0)
    script = (
        "import json,sys; r=json.load(sys.stdin); "
        "print('progress line'); "
        "print(json.dumps({'tx_hash': 'shell-' + r['extra']['memo'], 'signer_id': 'cli.testnet'}))"
    )
    client = Client(url, sleep=lambda _seconds: None)
    result = client.pay_and_scan({"title": "x"}, pay=shell_payer([sys.executable, "-c", script]))
    assert result.payment_response["payer"] == "cli.testnet"
    assert result.payment_response["transaction"].startswith("shell-aps2_mock_")
    assert mock.charges == 1


def test_shell_payer_failure_is_typed():
    pay = shell_payer([sys.executable, "-c", "import sys; sys.exit(3)"])
    with pytest.raises(X402Error):
        pay({"extra": {"memo": "m"}})
