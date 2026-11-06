"""AgentPay Sentinel x402 client (standard library only).

One call pays for a scan over HTTP 402, x402 v2:

    from aps_client import Client, shell_payer

    client = Client("http://127.0.0.1:8080")
    result = client.pay_and_scan(envelope, pay=shell_payer(["node", "cli.ts", "pay", ...]))
    print(result.report["decision"], result.payment["transaction"])

The flow is the near-tx-proof method (scheme ``exact``, ``paymentFlow``
``upfront``): request, receive 402 with a ``PAYMENT-REQUIRED`` header,
pay on NEAR (the pluggable ``pay`` callable), retry with a
``PAYMENT-SIGNATURE`` header carrying ``{network, intent_id, tx_hash,
signer_id}``, receive the report and a ``PAYMENT-RESPONSE`` header.

The payment step is a callable that receives the selected payment
requirement (a dict) and returns ``{"tx_hash": ..., "signer_id": ...}``.
The library never holds key material: signing happens in the callable,
for example the TypeScript reference payer (``clients/typescript``) via
:func:`shell_payer`.
"""

from __future__ import annotations

import base64
import json
import subprocess
import time
import urllib.error
import urllib.request
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any

__all__ = [
    "PAYMENT_FLOW",
    "SCHEME",
    "TRANSFER_METHOD",
    "Client",
    "PaidScan",
    "X402Error",
    "build_payment_payload",
    "decode_header",
    "encode_header",
    "select_requirement",
    "shell_payer",
]

X402_VERSION = 2
SCHEME = "exact"
TRANSFER_METHOD = "near-tx-proof"
PAYMENT_FLOW = "upfront"
HEADER_REQUIRED = "PAYMENT-REQUIRED"
HEADER_SIGNATURE = "PAYMENT-SIGNATURE"
HEADER_RESPONSE = "PAYMENT-RESPONSE"

Payer = Callable[[Mapping[str, Any]], Mapping[str, Any]]


class X402Error(Exception):
    """A payment or protocol failure, with the server's typed reason."""

    def __init__(
        self,
        message: str,
        *,
        status: int | None = None,
        reason: str | None = None,
        payment_response: Mapping[str, Any] | None = None,
        body: Any = None,
    ) -> None:
        super().__init__(message)
        self.status = status
        self.reason = reason
        self.payment_response = dict(payment_response or {})
        self.body = body


@dataclass
class PaidScan:
    """A paid scan: the response body and the decoded settlement."""

    body: dict
    payment_response: dict
    requirement: dict
    proof: dict
    attempts: int = 1
    headers: dict = field(default_factory=dict)

    @property
    def report(self) -> dict:
        return self.body.get("report", {})

    @property
    def payment(self) -> dict:
        return self.body.get("payment", {})


def encode_header(value: Any) -> str:
    """Base64 (standard alphabet) of compact JSON."""
    raw = json.dumps(value, separators=(",", ":")).encode("utf-8")
    return base64.b64encode(raw).decode("ascii")


def decode_header(raw: str) -> Any:
    """Decode a base64 JSON header (standard or URL-safe alphabet)."""
    text = raw.strip()
    padded = text + "=" * (-len(text) % 4)
    try:
        data = base64.b64decode(padded, altchars=b"-_" if ("-" in text or "_" in text) else None)
        return json.loads(data.decode("utf-8"))
    except (ValueError, UnicodeDecodeError) as exc:
        raise X402Error(f"header is not base64 JSON: {exc}") from exc


def select_requirement(payment_required: Mapping[str, Any], network: str | None = None) -> dict:
    """Pick the near-tx-proof ``accepts[]`` entry this client can pay.

    Entries with an unknown ``paymentFlow`` are skipped, as the core spec
    requires of clients.
    """
    if payment_required.get("x402Version") != X402_VERSION:
        raise X402Error("the server did not answer with an x402 v2 PaymentRequired")
    for entry in payment_required.get("accepts") or []:
        extra = entry.get("extra") or {}
        if entry.get("scheme") != SCHEME:
            continue
        if extra.get("assetTransferMethod") != TRANSFER_METHOD:
            continue
        if extra.get("paymentFlow", PAYMENT_FLOW) != PAYMENT_FLOW:
            continue
        if network is not None and entry.get("network") != network:
            continue
        if not extra.get("memo"):
            continue
        return dict(entry)
    raise X402Error("no near-tx-proof requirement this client can pay was offered")


def build_payment_payload(
    requirement: Mapping[str, Any],
    proof: Mapping[str, Any],
    resource: Mapping[str, Any] | None = None,
) -> dict:
    """A full x402 v2 PaymentPayload for a near-tx-proof payment."""
    tx_hash = str(proof.get("tx_hash") or "").strip()
    signer_id = str(proof.get("signer_id") or "").strip()
    if not tx_hash or not signer_id:
        raise X402Error("the payer must return non-empty tx_hash and signer_id")
    payload = {
        "x402Version": X402_VERSION,
        "accepted": dict(requirement),
        "payload": {
            "network": requirement["network"],
            "intent_id": requirement["extra"]["memo"],
            "tx_hash": tx_hash,
            "signer_id": signer_id,
        },
    }
    if resource:
        payload["resource"] = dict(resource)
    return payload


def shell_payer(command: Sequence[str], timeout: float = 180.0) -> Payer:
    """A payer that runs an external command (for example the TypeScript
    reference payer) with the requirement as JSON on stdin and reads
    ``{"tx_hash", "signer_id"}`` as JSON from the last stdout line.

    The command holds and uses the keys; nothing secret passes through
    this process.
    """

    def pay(requirement: Mapping[str, Any]) -> Mapping[str, Any]:
        completed = subprocess.run(
            list(command),
            input=json.dumps(requirement),
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        if completed.returncode != 0:
            tail = (completed.stderr or "").strip().splitlines()[-3:]
            raise X402Error(
                f"the payer command failed (exit {completed.returncode}): {' | '.join(tail)}"
            )
        lines = [line for line in completed.stdout.splitlines() if line.strip()]
        if not lines:
            raise X402Error("the payer command printed nothing")
        try:
            result = json.loads(lines[-1])
        except ValueError as exc:
            raise X402Error("the payer command did not print JSON") from exc
        return result

    return pay


class Client:
    """A minimal x402 client for the Sentinel ``/v2/scan`` resource."""

    def __init__(
        self,
        base_url: str,
        *,
        timeout: float = 60.0,
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self._sleep = sleep

    def _post(self, path: str, body: Any, headers: Mapping[str, str] | None = None):
        data = json.dumps(body).encode("utf-8")
        request = urllib.request.Request(
            self.base_url + path,
            data=data,
            method="POST",
            headers={"Content-Type": "application/json", **dict(headers or {})},
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return response.status, dict(response.headers.items()), _json(response.read())
        except urllib.error.HTTPError as error:
            return error.code, dict(error.headers.items()), _json(error.read())

    def request_requirement(self, envelope: Mapping[str, Any], path: str = "/v2/scan"):
        """POST without payment; return (PaymentRequired, response body)."""
        status, headers, body = self._post(path, envelope)
        if status != 402:
            raise X402Error(
                f"expected 402 Payment Required, got {status}",
                status=status,
                body=body,
            )
        raw = _header(headers, HEADER_REQUIRED)
        if raw is None:
            raise X402Error("402 without a PAYMENT-REQUIRED header", status=status, body=body)
        return decode_header(raw), body

    def submit(
        self,
        envelope: Mapping[str, Any],
        payment_payload: Mapping[str, Any],
        path: str = "/v2/scan",
        *,
        max_pending_retries: int = 40,
        pending_delay: float | None = None,
    ):
        """Retry with ``PAYMENT-SIGNATURE`` until settled or failed.

        ``settlement_pending`` (the transaction is not final yet) is
        retried with the same proof, honoring ``Retry-After``.
        Returns (status, headers, body, attempts).
        """
        signature = encode_header(payment_payload)
        attempts = 0
        while True:
            attempts += 1
            status, headers, body = self._post(path, envelope, {HEADER_SIGNATURE: signature})
            if status == 200:
                return status, headers, body, attempts
            raw = _header(headers, HEADER_RESPONSE)
            settlement = decode_header(raw) if raw else {}
            reason = settlement.get("errorReason") or _error_code(body)
            if status == 402 and reason == "settlement_pending" and attempts <= max_pending_retries:
                delay = pending_delay
                if delay is None:
                    retry_after = _header(headers, "Retry-After")
                    delay = float(retry_after) if retry_after else 2.0
                self._sleep(delay)
                continue
            raise X402Error(
                f"payment failed ({status} {reason}): {_error_message(body)}",
                status=status,
                reason=reason,
                payment_response=settlement,
                body=body,
            )

    def pay_and_scan(
        self,
        envelope: Mapping[str, Any],
        pay: Payer,
        *,
        network: str | None = None,
        max_pending_retries: int = 40,
        pending_delay: float | None = None,
    ) -> PaidScan:
        """Request, pay, retry: one paid scan.

        ``pay`` receives the selected requirement and must return
        ``{"tx_hash", "signer_id"}`` for a transfer of exactly
        ``requirement["amount"]`` of ``requirement["asset"]`` to
        ``requirement["payTo"]`` with memo ``requirement["extra"]["memo"]``.
        """
        payment_required, _ = self.request_requirement(envelope)
        requirement = select_requirement(payment_required, network)
        proof = dict(pay(requirement))
        payload = build_payment_payload(requirement, proof, payment_required.get("resource"))
        _, headers, body, attempts = self.submit(
            envelope,
            payload,
            max_pending_retries=max_pending_retries,
            pending_delay=pending_delay,
        )
        raw = _header(headers, HEADER_RESPONSE)
        settlement = decode_header(raw) if raw else {}
        if not settlement.get("success"):
            raise X402Error("200 without a successful PAYMENT-RESPONSE", status=200, body=body)
        return PaidScan(
            body=body,
            payment_response=settlement,
            requirement=requirement,
            proof=proof,
            attempts=attempts,
            headers=headers,
        )


def _json(raw: bytes) -> Any:
    if not raw:
        return {}
    try:
        return json.loads(raw.decode("utf-8"))
    except ValueError:
        return {"raw": raw.decode("utf-8", "replace")}


def _header(headers: Mapping[str, str], name: str) -> str | None:
    lowered = name.lower()
    for key, value in headers.items():
        if key.lower() == lowered:
            return value
    return None


def _error_code(body: Any) -> str | None:
    if isinstance(body, dict):
        return (body.get("error") or {}).get("code")
    return None


def _error_message(body: Any) -> str:
    if isinstance(body, dict):
        return str((body.get("error") or {}).get("message") or "")
    return ""
