"""aps-triage-model: the Stage B advisory judge of the triage engine v2.

Stdlib only. Calls an OpenAI-compatible chat-completions API (NEAR AI
Cloud by default) at temperature 0 with a pinned model id, a versioned and
hashed prompt template, and a strict JSON output schema. Schema
violations get one retry, then fail closed. Responses are cached by
(envelope hash, ruleset hash, model id, prompt hash).

The model is advisory: the scanner's combiner never lets it raise a
decision above the deterministic Stage A gates, and it never moves money.

Live calls happen only when APS_LIVE_MODEL=1 and NEARAI_API_KEY are set;
the key is read from the environment and never logged.
"""

from . import _scanner  # noqa: F401  (side effect: makes aps_scanner importable)

__version__ = "0.1.0"
