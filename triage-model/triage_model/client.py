"""OpenAI-compatible chat-completions client (stdlib urllib only).

Defaults follow docs/research/near-ai-cloud.md (NEAR AI Cloud, accessed
2026-10-21): the gateway base URL ``https://cloud-api.near.ai/v1``,
``POST /chat/completions``, ``Authorization: Bearer <key>``, and the
``x-no-aliasing: true`` header so the pinned canonical model id is not
rewritten by alias resolution. Requests are non-streaming, temperature 0,
with a strict ``json_schema`` response format.

The API key is held privately and never appears in an exception message,
a repr, or a log line. HTTP errors surface as the status code only (the
response body may echo the request).
"""

import json
import urllib.error
import urllib.request
from dataclasses import dataclass

DEFAULT_BASE_URL = "https://cloud-api.near.ai/v1"
DEFAULT_TIMEOUT_SECS = 60
# Every TEE-listed chat model in the catalogue reasons before it answers,
# and the reasoning tokens count against max_tokens. With 1024 tokens and
# the default effort, all of them stopped at the limit before writing any
# content (2026-11-12). With effort "low" a verdict took about 150 tokens.
DEFAULT_MAX_TOKENS = 4096
DEFAULT_REASONING_EFFORT = "low"
REASONING_EFFORTS = ("low", "medium", "high", "none")
USER_AGENT = "aps-triage-model/0.1"
# json_schema: strict structured output (default); json_object: JSON mode
# for models without structured outputs; none: plain text, strict parse.
RESPONSE_FORMATS = ("json_schema", "json_object", "none")


class ModelCallError(RuntimeError):
    """The model API call failed (transport, HTTP status, or response shape)."""


class ModelTimeout(ModelCallError):
    """The model API call timed out."""


@dataclass(frozen=True)
class Completion:
    content: str
    usage: dict | None = None
    request_id: str | None = None


def urllib_transport(url: str, headers: dict, body: bytes, timeout: float):
    """POST and return (status, response headers, body bytes)."""
    request = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, dict(response.headers), response.read()
    except urllib.error.HTTPError as err:
        raise ModelCallError(f"model API returned HTTP {err.code}") from None
    except TimeoutError as err:  # socket.timeout is an alias since 3.10
        raise ModelTimeout("model API timed out") from err
    except urllib.error.URLError as err:
        if isinstance(err.reason, TimeoutError):
            raise ModelTimeout("model API timed out") from None
        raise ModelCallError("model API is unreachable") from None


class ChatClient:
    """A minimal chat-completions client. ``transport`` is injectable for tests."""

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        timeout: float = DEFAULT_TIMEOUT_SECS,
        max_tokens: int = DEFAULT_MAX_TOKENS,
        response_format: str = "json_schema",
        reasoning_effort: str = DEFAULT_REASONING_EFFORT,
        transport=urllib_transport,
    ):
        if not api_key:
            raise ValueError("an API key is required for live calls")
        if not base_url.startswith("https://"):
            raise ValueError("the model API base URL must be https")
        self._api_key = api_key
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.max_tokens = max_tokens
        if response_format not in RESPONSE_FORMATS:
            raise ValueError(f"response_format must be one of {RESPONSE_FORMATS}")
        self.response_format = response_format
        if reasoning_effort not in REASONING_EFFORTS:
            raise ValueError(f"reasoning_effort must be one of {REASONING_EFFORTS}")
        # "none" sends no reasoning_effort field (the provider's default).
        self.reasoning_effort = reasoning_effort
        self._transport = transport

    def __repr__(self):
        return f"ChatClient(base_url={self.base_url!r}, api_key=<redacted>)"

    def request_body(self, messages, model_id: str, response_schema: dict) -> dict:
        body = {
            "model": model_id,
            "messages": messages,
            "temperature": 0,
            "stream": False,
            "max_tokens": self.max_tokens,
        }
        if self.reasoning_effort != "none":
            body["reasoning_effort"] = self.reasoning_effort
        if self.response_format == "json_schema":
            body["response_format"] = {
                "type": "json_schema",
                "json_schema": {
                    "name": "triage_verdict",
                    "strict": True,
                    "schema": response_schema,
                },
            }
        elif self.response_format == "json_object":
            body["response_format"] = {"type": "json_object"}
        return body

    def complete(self, messages, *, model_id: str, response_schema: dict) -> Completion:
        body = json.dumps(self.request_body(messages, model_id, response_schema)).encode("utf-8")
        headers = {
            "Authorization": f"Bearer {self._api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
            "x-no-aliasing": "true",
            "User-Agent": USER_AGENT,
        }
        status, response_headers, raw = self._transport(
            f"{self.base_url}/chat/completions", headers, body, self.timeout
        )
        if status != 200:
            raise ModelCallError(f"model API returned HTTP {status}")
        try:
            payload = json.loads(raw.decode("utf-8"))
            content = payload["choices"][0]["message"]["content"]
        except (UnicodeDecodeError, json.JSONDecodeError, KeyError, IndexError, TypeError):
            raise ModelCallError("model API response has an unexpected shape") from None
        if not isinstance(content, str):
            raise ModelCallError("model API response has no text content")
        usage = payload.get("usage") if isinstance(payload.get("usage"), dict) else None
        request_id = None
        for name, value in (response_headers or {}).items():
            if name.lower() == "x-request-id":
                request_id = str(value)
        return Completion(content=content, usage=usage, request_id=request_id)
