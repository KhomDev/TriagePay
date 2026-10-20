"""OpenAPI conformance: docs/openapi.json must describe exactly the
routes the axum router serves, with the single error shape everywhere.

Run from the repository root: python -m pytest tests -q
"""

import json
import pathlib

SPEC = pathlib.Path(__file__).resolve().parent.parent / "docs" / "openapi.json"

EXPECTED_PATHS = {
    "/health",
    "/accounts/{account_id}",
    "/accounts/{account_id}/deposits",
    "/reservations",
    "/jobs/{job_id}",
    "/jobs/{job_id}/start",
    "/jobs/{job_id}/settlements",
    "/jobs/{job_id}/scan",
    "/jobs/{job_id}/refunds",
    "/payment/intents",
    "/payment/intents/{intent_id}",
    "/payment/intents/{intent_id}/observations",
    "/payment/intents/{intent_id}/refunds",
}


def test_spec_loads_with_required_metadata():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    assert spec["openapi"].startswith("3.")
    assert spec["info"]["title"]
    assert spec["info"]["version"]


def test_spec_covers_exactly_the_router_paths():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    assert set(spec["paths"]) == EXPECTED_PATHS


def test_every_operation_documents_non_2xx_with_the_error_body():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    for path, item in spec["paths"].items():
        for method, operation in item.items():
            if method not in ("get", "post", "put", "delete", "patch"):
                continue
            # The liveness probe has no failure contract by design.
            if path == "/health":
                continue
            statuses = {
                code
                for code in operation.get("responses", {})
                if not code.startswith("2")
            }
            assert statuses, f"{method} {path} documents no failure responses"
            for code in statuses:
                response = operation["responses"][code]
                schema = response.get("content", {}).get("application/json", {}).get("schema")
                assert schema is not None, f"{method} {path} {code} has no JSON body"
                assert schema.get("$ref") == "#/components/schemas/ErrorBody" or (
                    schema.get("properties", {}).get("error")
                ), f"{method} {path} {code} is not the standard error shape"


def test_money_schemas_carry_atomic_and_decimal():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    amount = spec["components"]["schemas"]["AmountJson"]
    assert amount["required"] == ["atomic", "decimal"]
    balances = spec["components"]["schemas"]["Balances"]
    assert balances["additionalProperties"] is False
