"""The chat client over a fake transport (no network): request shape, key
hygiene, error mapping."""

import json
import urllib.error

import pytest

from triage_model import client as client_module
from triage_model.client import ChatClient, ModelCallError, ModelTimeout
from triage_model.schema import OUTPUT_SCHEMA

FAKE_KEY = "zz-test-key-not-a-secret-0001"


class FakeTransport:
    def __init__(self, status=200, body=None, headers=None, raises=None):
        self.status = status
        self.body = body if body is not None else {"choices": [{"message": {"content": "{}"}}]}
        self.headers = headers or {"X-Request-Id": "req-1"}
        self.raises = raises
        self.calls = []

    def __call__(self, url, headers, body, timeout):
        self.calls.append({"url": url, "headers": headers, "body": json.loads(body), "timeout": timeout})
        if self.raises:
            raise self.raises
        raw = self.body if isinstance(self.body, bytes) else json.dumps(self.body).encode()
        return self.status, self.headers, raw


def make(transport, **kwargs):
    return ChatClient(FAKE_KEY, transport=transport, **kwargs)


def test_request_shape_matches_the_documented_api():
    transport = FakeTransport(
        body={"choices": [{"message": {"content": "{\"a\": 1}"}}], "usage": {"prompt_tokens": 9}}
    )
    completion = make(transport).complete(
        [{"role": "user", "content": "hi"}], model_id="vendor/model-x", response_schema=OUTPUT_SCHEMA
    )
    call = transport.calls[0]
    assert call["url"] == "https://cloud-api.near.ai/v1/chat/completions"
    assert call["headers"]["Authorization"] == f"Bearer {FAKE_KEY}"
    assert call["headers"]["x-no-aliasing"] == "true"
    body = call["body"]
    assert body["model"] == "vendor/model-x"
    assert body["temperature"] == 0
    assert body["stream"] is False
    assert body["response_format"]["type"] == "json_schema"
    assert body["response_format"]["json_schema"]["strict"] is True
    # Reasoning models spend max_tokens on hidden reasoning first; the
    # defaults leave room for the answer (observed 2026-11-12).
    assert body["reasoning_effort"] == "low"
    assert body["max_tokens"] == 4096
    assert completion.content == '{"a": 1}'
    assert completion.usage == {"prompt_tokens": 9}
    assert completion.request_id == "req-1"


def test_key_never_appears_in_repr_or_errors():
    transport = FakeTransport(status=401)
    chat = make(transport)
    assert FAKE_KEY not in repr(chat)
    with pytest.raises(ModelCallError) as excinfo:
        chat.complete([], model_id="m", response_schema={})
    assert FAKE_KEY not in str(excinfo.value)
    assert "401" in str(excinfo.value)


@pytest.mark.parametrize(
    "body",
    [b"not json", {"choices": []}, {"choices": [{"message": {}}]}, {"choices": [{"message": {"content": 5}}]}],
)
def test_unexpected_shapes_raise(body):
    with pytest.raises(ModelCallError):
        make(FakeTransport(body=body)).complete([], model_id="m", response_schema={})


def test_transport_timeout_maps_to_model_timeout():
    with pytest.raises(ModelTimeout):
        make(FakeTransport(raises=ModelTimeout("model API timed out"))).complete(
            [], model_id="m", response_schema={}
        )


def test_urllib_transport_maps_errors_without_bodies(monkeypatch):
    def raise_http(*_a, **_k):
        raise urllib.error.HTTPError("https://x.example/v1", 500, "secret body", {}, None)

    monkeypatch.setattr(client_module.urllib.request, "urlopen", raise_http)
    with pytest.raises(ModelCallError) as excinfo:
        client_module.urllib_transport("https://x.example/v1", {}, b"{}", 1)
    assert str(excinfo.value) == "model API returned HTTP 500"

    def raise_timeout(*_a, **_k):
        raise urllib.error.URLError(TimeoutError("timed out"))

    monkeypatch.setattr(client_module.urllib.request, "urlopen", raise_timeout)
    with pytest.raises(ModelTimeout):
        client_module.urllib_transport("https://x.example/v1", {}, b"{}", 1)


def test_construction_guards():
    with pytest.raises(ValueError):
        ChatClient("")
    with pytest.raises(ValueError):
        ChatClient(FAKE_KEY, base_url="http://cloud-api.near.ai/v1")
    with pytest.raises(ValueError):
        ChatClient(FAKE_KEY, response_format="xml")
    with pytest.raises(ValueError):
        ChatClient(FAKE_KEY, reasoning_effort="maximum")


def test_reasoning_effort_none_sends_no_field():
    body = ChatClient(FAKE_KEY, reasoning_effort="none").request_body([], "m", OUTPUT_SCHEMA)
    assert "reasoning_effort" not in body


def test_output_schema_carries_no_keyword_the_grammar_engine_refuses():
    # NEAR AI Cloud answered HTTP 400 "Unimplemented keys: [uniqueItems]"
    # for the whole request; validate_output enforces distinct items.
    assert "uniqueItems" not in json.dumps(OUTPUT_SCHEMA)


@pytest.mark.parametrize(
    "mode, expected",
    [("json_object", {"type": "json_object"}), ("none", None)],
)
def test_response_format_modes(mode, expected):
    body = ChatClient(FAKE_KEY, response_format=mode).request_body([], "m", OUTPUT_SCHEMA)
    assert body.get("response_format") == expected
    assert body["temperature"] == 0
