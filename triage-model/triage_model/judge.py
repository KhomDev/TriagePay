"""The Stage B judge: prompt, model call, strict validation, one retry, cache.

``judge(request, backend=..., model_id=...)`` returns the dict contract the
scanner's engine_v2 re-validates:

- ok: bountiable_probability, severity_call, taxonomy_hits,
  missing_evidence, public_rationale, response_digest, model_id,
  prompt_hash;
- fail_closed: failure in {timeout, transport_error, schema_violation},
  plus model_id and prompt_hash.

A schema violation gets exactly one retry; a second violation fails
closed. Timeouts and transport errors fail closed without a retry. The
combiner maps every fail-closed result to needs_evidence.
"""

import hashlib
import os

from . import config
from .backends import CassetteBackend, LiveBackend
from .cache import DirectoryCache, cache_key
from .client import ChatClient, ModelCallError, ModelTimeout
from .prompt import PROMPT_HASH, build_messages, retry_messages
from .schema import OUTPUT_SCHEMA, parse_output

STATUS_OK = "ok"
STATUS_FAIL_CLOSED = "fail_closed"


def _fail_closed(failure, model_id):
    return {
        "status": STATUS_FAIL_CLOSED,
        "failure": failure,
        "model_id": model_id,
        "prompt_hash": PROMPT_HASH,
    }


def judge(request: dict, *, backend, model_id: str = config.DEFAULT_MODEL_ID, cache=None):
    key = cache_key(request["envelope_hash"], request["ruleset_hash"], model_id, PROMPT_HASH)
    if cache is not None:
        cached = cache.get(key)
        if cached is not None:
            return cached
    messages = build_messages(request)
    for attempt in range(2):
        try:
            completion = backend.complete(
                messages, model_id=model_id, response_schema=OUTPUT_SCHEMA
            )
        except ModelTimeout:
            return _fail_closed("timeout", model_id)
        except ModelCallError:
            return _fail_closed("transport_error", model_id)
        value, errors = parse_output(completion.content)
        if not errors:
            result = {
                "status": STATUS_OK,
                "bountiable_probability": float(value["bountiable_probability"]),
                "severity_call": value["severity_call"],
                "taxonomy_hits": list(value["taxonomy_hits"]),
                "missing_evidence": list(value["missing_evidence"]),
                "public_rationale": value["public_rationale"],
                "response_digest": hashlib.sha256(
                    completion.content.encode("utf-8")
                ).hexdigest(),
                "model_id": model_id,
                "prompt_hash": PROMPT_HASH,
            }
            if cache is not None:
                cache.put(key, result)
            return result
        if attempt == 0:
            messages = retry_messages(messages)
    return _fail_closed("schema_violation", model_id)


def make_judge(backend, *, model_id: str = config.DEFAULT_MODEL_ID, cache=None):
    """A one-argument judge callable for aps_scanner.engine_v2.triage."""

    def bound(request):
        return judge(request, backend=backend, model_id=model_id, cache=cache)

    bound.model_id = model_id
    return bound


def judge_from_env(environ=None):
    """The configured judge, or None when no model is configured.

    - APS_LIVE_MODEL=1 and NEARAI_API_KEY set: live calls (key from the
      environment, never logged);
    - else APS_TRIAGE_CASSETTES set: replay-only from that directory;
    - else None (the caller treats Stage B as unavailable).

    APS_TRIAGE_CACHE_DIR, when set, enables the on-disk response cache.
    """
    environ = os.environ if environ is None else environ
    model_id = config.model_id(environ)
    cache_dir = environ.get(config.CACHE_DIR_ENV, "").strip()
    cache = DirectoryCache(cache_dir) if cache_dir else None
    if config.live_enabled(environ):
        client = ChatClient(
            environ[config.API_KEY_ENV].strip(),
            base_url=config.base_url(environ),
        )
        return make_judge(LiveBackend(client), model_id=model_id, cache=cache)
    cassettes = environ.get(config.CASSETTES_ENV, "").strip()
    if cassettes:
        return make_judge(CassetteBackend(cassettes), model_id=model_id, cache=cache)
    return None
