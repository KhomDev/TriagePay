"""Environment configuration (names only; values are never logged).

- APS_LIVE_MODEL=1 together with NEARAI_API_KEY enables live calls;
- APS_TRIAGE_MODEL_ID overrides the pinned model id;
- APS_NEARAI_BASE_URL overrides the gateway base URL (https only);
- APS_TRIAGE_CASSETTES names a cassette directory for replay;
- APS_TRIAGE_CACHE_DIR names the response-cache directory.

The default model is a pinned canonical id from the NEAR AI Cloud
catalogue recorded in docs/research/near-ai-cloud.md (2026-10-21): a
TEE-backed ("vllm", verifiable) model that lists structured outputs. The
spike that would confirm or replace it is pending the API key
(docs/engine/model-selection.md).
"""

from .client import DEFAULT_BASE_URL

LIVE_ENV = "APS_LIVE_MODEL"
API_KEY_ENV = "NEARAI_API_KEY"
MODEL_ID_ENV = "APS_TRIAGE_MODEL_ID"
BASE_URL_ENV = "APS_NEARAI_BASE_URL"
CASSETTES_ENV = "APS_TRIAGE_CASSETTES"
CACHE_DIR_ENV = "APS_TRIAGE_CACHE_DIR"

DEFAULT_MODEL_ID = "z-ai/glm-5.3-flash"


def live_enabled(environ) -> bool:
    return environ.get(LIVE_ENV, "") == "1" and bool(environ.get(API_KEY_ENV, "").strip())


def model_id(environ) -> str:
    return environ.get(MODEL_ID_ENV, "").strip() or DEFAULT_MODEL_ID


def base_url(environ) -> str:
    return environ.get(BASE_URL_ENV, "").strip() or DEFAULT_BASE_URL
