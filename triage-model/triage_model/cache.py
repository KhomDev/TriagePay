"""Response cache keyed by (envelope hash, ruleset hash, model id, prompt hash).

Only validated ("ok") Stage B results are stored, so a replay of a cached
finding returns a bit-identical result without a model call. Failures
(timeouts, schema violations) are never cached.
"""

import hashlib
import json
import pathlib


def _canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def cache_key(envelope_hash: str, ruleset_hash: str, model_id: str, prompt_hash: str) -> str:
    parts = {
        "envelope_hash": envelope_hash,
        "ruleset_hash": ruleset_hash,
        "model_id": model_id,
        "prompt_hash": prompt_hash,
    }
    return hashlib.sha256(_canonical(parts).encode("utf-8")).hexdigest()


class MemoryCache:
    def __init__(self):
        self._entries = {}

    def get(self, key):
        raw = self._entries.get(key)
        return json.loads(raw) if raw is not None else None

    def put(self, key, value):
        self._entries[key] = _canonical(value)


class DirectoryCache:
    """One canonical JSON file per key."""

    def __init__(self, directory):
        self.directory = pathlib.Path(directory)

    def _path(self, key):
        return self.directory / f"{key}.json"

    def get(self, key):
        try:
            return json.loads(self._path(key).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return None

    def put(self, key, value):
        self.directory.mkdir(parents=True, exist_ok=True)
        self._path(key).write_text(_canonical(value) + "\n", encoding="utf-8", newline="\n")
