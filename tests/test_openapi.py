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
    "/v2/scan",
    "/x402/supported",
    "/x402/verify",
    "/x402/settle",
}

ROUTER = pathlib.Path(__file__).resolve().parent.parent / "crates" / "api" / "src" / "lib.rs"


def _router_paths():
    """Route literals registered in crates/api/src/lib.rs."""
    import re

    text = ROUTER.read_text(encoding="utf-8")
    paths = set(re.findall(r'\.route\(\s*"([^"]+)"', text))
    # The x402 resource is registered through its constant.
    if "x402::RESOURCE_PATH" in text:
        paths.add("/v2/scan")
    return paths


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
            # The liveness probe and the static facilitator capability
            # list have no failure contract by design.
            if path in ("/health", "/x402/supported"):
                continue
            statuses = {code for code in operation.get("responses", {}) if not code.startswith("2")}
            assert statuses, f"{method} {path} documents no failure responses"
            for code in statuses:
                response = operation["responses"][code]
                schema = response.get("content", {}).get("application/json", {}).get("schema")
                assert schema is not None, f"{method} {path} {code} has no JSON body"
                ref = schema.get("$ref", "")
                if (
                    ref.startswith("#/components/schemas/")
                    and ref != "#/components/schemas/ErrorBody"
                ):
                    schema = spec["components"]["schemas"][ref.rsplit("/", 1)[1]]
                assert schema.get("$ref") == "#/components/schemas/ErrorBody" or (
                    schema.get("properties", {}).get("error")
                ), f"{method} {path} {code} is not the standard error shape"


def test_expected_paths_are_exactly_the_router_routes():
    assert _router_paths() == EXPECTED_PATHS


def test_x402_resource_documents_the_v2_headers():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    operation = spec["paths"]["/v2/scan"]["post"]
    params = {p["name"] for p in operation["parameters"] if p.get("in") == "header"}
    assert params == {"PAYMENT-SIGNATURE"}
    assert set(operation["responses"]["402"]["headers"]) >= {
        "PAYMENT-REQUIRED",
        "PAYMENT-RESPONSE",
    }
    assert "PAYMENT-RESPONSE" in operation["responses"]["200"]["headers"]
    # The settlement_pending reason is documented on the 402.
    assert "settlement_pending" in operation["responses"]["402"]["description"]


def test_facilitator_endpoints_follow_the_core_spec_shapes():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    paths = spec["paths"]
    assert set(
        paths["/x402/supported"]["get"]["responses"]["200"]["content"]["application/json"][
            "schema"
        ]["required"]
    ) == {"kinds", "extensions", "signers"}
    verify = paths["/x402/verify"]["post"]["responses"]["200"]
    assert verify["content"]["application/json"]["schema"]["required"] == ["isValid"]
    settle = paths["/x402/settle"]["post"]["responses"]["200"]
    assert set(settle["content"]["application/json"]["schema"]["required"]) == {
        "success",
        "transaction",
        "network",
    }
    request = spec["components"]["schemas"]["FacilitatorRequest"]
    assert set(request["required"]) == {"x402Version", "paymentPayload", "paymentRequirements"}


def test_money_schemas_carry_atomic_and_decimal():
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    amount = spec["components"]["schemas"]["AmountJson"]
    assert amount["required"] == ["atomic", "decimal"]
    balances = spec["components"]["schemas"]["Balances"]
    assert balances["additionalProperties"] is False
