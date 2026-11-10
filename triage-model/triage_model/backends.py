"""Model backends: live, scripted (offline tests), cassette replay, recording.

Every backend has ``complete(messages, *, model_id, response_schema) ->
Completion``. Cassettes are keyed by the sha256 of the canonical request
(model id, messages, schema) and store only that digest, the model id and
the reply content, never the request text. Only synthetic or
public-benchmark inputs may be recorded.
"""

import hashlib
import json
import pathlib

from .client import ChatClient, Completion, ModelCallError


def _canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def request_digest(messages, model_id: str, response_schema: dict) -> str:
    payload = {"model_id": model_id, "messages": messages, "schema": response_schema}
    return hashlib.sha256(_canonical(payload).encode("utf-8")).hexdigest()


class LiveBackend:
    def __init__(self, client: ChatClient):
        self.client = client

    def complete(self, messages, *, model_id, response_schema):
        return self.client.complete(messages, model_id=model_id, response_schema=response_schema)


class ScriptedBackend:
    """Offline fake model: replies come from a list (in order) or a callable.

    A reply that is an Exception instance is raised instead of returned.
    """

    def __init__(self, replies):
        self._replies = replies if callable(replies) else list(replies)
        self.calls = []

    def complete(self, messages, *, model_id, response_schema):
        self.calls.append({"messages": messages, "model_id": model_id})
        if callable(self._replies):
            reply = self._replies(messages)
        elif self._replies:
            reply = self._replies.pop(0)
        else:
            raise ModelCallError("the scripted model has no reply left")
        if isinstance(reply, Exception):
            raise reply
        if isinstance(reply, (dict, list)):
            reply = json.dumps(reply)
        return Completion(content=reply)


class CassetteBackend:
    """Replay-only backend over a cassette directory (a miss fails closed)."""

    def __init__(self, directory):
        self.directory = pathlib.Path(directory)

    def path_for(self, digest: str) -> pathlib.Path:
        return self.directory / f"{digest}.json"

    def complete(self, messages, *, model_id, response_schema):
        digest = request_digest(messages, model_id, response_schema)
        path = self.path_for(digest)
        try:
            cassette = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            raise ModelCallError("no cassette recorded for this request") from None
        if cassette.get("request_digest") != digest or cassette.get("model_id") != model_id:
            raise ModelCallError("cassette does not match this request")
        content = cassette.get("content")
        if not isinstance(content, str):
            raise ModelCallError("cassette has no content")
        return Completion(content=content, usage=cassette.get("usage"))


class RecordingBackend:
    """Wraps another backend and writes each reply as a cassette."""

    def __init__(self, inner, directory):
        self.inner = inner
        self.replay = CassetteBackend(directory)

    def complete(self, messages, *, model_id, response_schema):
        completion = self.inner.complete(
            messages, model_id=model_id, response_schema=response_schema
        )
        digest = request_digest(messages, model_id, response_schema)
        self.replay.directory.mkdir(parents=True, exist_ok=True)
        cassette = {
            "request_digest": digest,
            "model_id": model_id,
            "content": completion.content,
            "usage": completion.usage,
        }
        self.replay.path_for(digest).write_text(
            _canonical(cassette) + "\n", encoding="utf-8", newline="\n"
        )
        return completion
