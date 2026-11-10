#!/usr/bin/env bash
# TriagePay demo: prepaid scan, paid, executed, settled, refunded.
#
# Starts the API on an ephemeral port and walks the full product flow:
# provider-observed funding, reservation, scan, settlement against the
# report hash, an invalid-envelope rejection, and a refund.
#
# Requirements: cargo, python3 (for the scanner and JSON extraction).
set -euo pipefail

PORT="${APS_DEMO_PORT:-$((8800 + RANDOM % 1100))}"
RUN_KEY="$(date +%s)"
BASE="http://127.0.0.1:${PORT}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "== TriagePay demo =="
echo

echo "-- starting aps-api on port ${PORT}"
cd "${REPO_ROOT}"
APS_PORT="${PORT}" cargo run -q -p aps-api --bin aps-api &
SERVER_PID=$!
trap 'kill ${SERVER_PID} 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
  if curl -sf "${BASE}/health" >/dev/null 2>&1; then break; fi
  sleep 1
done
echo "-- server is up: $(curl -s ${BASE}/health)"
echo

jqget() { python3 -c "import sys, json; print(json.load(sys.stdin)[sys.argv[1]])" "$1"; }

echo "-- 1. create a payment intent: 5 USDC for account demo-agent"
INTENT=$(curl -s -X POST "${BASE}/payment/intents" -H "content-type: application/json" -d '{"account_id":"demo-agent","asset_id":"asset.usdc.test","amount_atomic":"5000000","expires_in_secs":3600}')
echo "${INTENT}" | python3 -m json.tool
INTENT_ID=$(echo "${INTENT}" | jqget intent_id)
echo

echo "-- 2. observe the intent: the provider confirms, the account is funded"
OBS=$(curl -s -X POST "${BASE}/payment/intents/${INTENT_ID}/observations")
echo "${OBS}" | python3 -m json.tool
echo

echo "-- 3. reserve one scan (scan.v1 = 1 USDC) and start the job"
RES=$(curl -s -X POST "${BASE}/reservations" -H "content-type: application/json" -d '{"account_id":"demo-agent","sku":"scan.v1","amount_atomic":"1000000","idempotency_key":"demo-r1-${RUN_KEY}"}')
JOB_ID=$(echo "${RES}" | jqget job_id)
echo "reserved job: ${JOB_ID}"
curl -s -X POST "${BASE}/jobs/${JOB_ID}/start" | python3 -m json.tool
echo

echo "-- 4. scan a finding: the scanner runs, the report settles the job"
SCAN=$(curl -s -X POST "${BASE}/jobs/${JOB_ID}/scan" -H "content-type: application/json" -d @"${REPO_ROOT}/demo/envelope.json")
echo "${SCAN}" | python3 -m json.tool
echo

echo "-- 5. an invalid envelope is rejected with 400 and never charges"
INVALID=$(python3 - "${REPO_ROOT}/demo/envelope.json" <<'PYEOF'
import json, sys
with open(sys.argv[1]) as handle:
    envelope = json.load(handle)
envelope["title"] = "x" * 201
print(json.dumps(envelope))
PYEOF
)
RES2=$(curl -s -X POST "${BASE}/reservations" -H "content-type: application/json" -d '{"account_id":"demo-agent","sku":"scan.v1","amount_atomic":"1000000","idempotency_key":"demo-r2-${RUN_KEY}"}')
JOB2=$(echo "${RES2}" | jqget job_id)
curl -s -X POST "${BASE}/jobs/${JOB2}/start" >/dev/null
INVALID_CODE=$(curl -s -o /tmp/aps-demo-invalid.json -w "%{http_code}" -X POST "${BASE}/jobs/${JOB2}/scan" -H "content-type: application/json" -d "${INVALID}")
echo "invalid envelope -> HTTP ${INVALID_CODE} (expected 400)"
cat /tmp/aps-demo-invalid.json | python3 -m json.tool
echo

echo "-- 6. refund the untouched reservation (simulated timeout)"
curl -s -X POST "${BASE}/jobs/${JOB2}/refunds" -H "content-type: application/json" -d '{"reason":"demo: simulated timeout"}' | python3 -m json.tool
echo

echo "-- final balances:"
curl -s "${BASE}/accounts/demo-agent" | python3 -m json.tool
echo
echo "== demo complete =="

