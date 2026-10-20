#!/usr/bin/env bash
# Operator-gated mainnet E2E for the NEAR payment adapter (Linux/macOS).
#
# This script performs ONE on-chain action, and it is yours, not the
# script's: the deposit transfer happens in YOUR wallet when you approve
# it. The script itself only talks to the local API and verifies the
# result on chain through the adapter. See docs/integrations/near.md.
#
# Required environment (see .env.example):
#   APS_PROVIDER=near NEAR_NETWORK NEAR_GATEWAY_ACCOUNT NEAR_ASSET_CONTRACT
# Optional: NEAR_RPC_URL, APS_PORT, ACCOUNT (default near-demo),
#   AMOUNT_ATOMIC (default 1000000).

set -euo pipefail

PORT="${APS_PORT:-8080}"
BASE="http://127.0.0.1:${PORT}"
ACCOUNT="${ACCOUNT:-near-demo}"
AMOUNT="${AMOUNT_ATOMIC:-1000000}"

if [ "${APS_PROVIDER:-}" != "near" ]; then
  echo "APS_PROVIDER must be 'near' (with NEAR_NETWORK, NEAR_GATEWAY_ACCOUNT,"
  echo "NEAR_ASSET_CONTRACT set) - see .env.example and docs/integrations/near.md."
  exit 2
fi

echo "== 1. starting aps-api with the near provider =="
cargo run -q -p aps-api 2>target/near-demo-server.log &
SERVER_PID=$!
trap 'kill "${SERVER_PID}" 2>/dev/null || true' EXIT

for _ in $(seq 1 40); do
  sleep 0.5
  if curl -sf "${BASE}/health" >/dev/null 2>&1; then break; fi
done
curl -sf "${BASE}/health" >/dev/null || { echo "aps-api did not become healthy"; exit 1; }

echo "== 2. creating the deposit intent =="
INTENT=$(curl -sf -X POST "${BASE}/payment/intents" -H 'Content-Type: application/json' \
  -d "{\"account_id\":\"${ACCOUNT}\",\"asset_id\":\"${NEAR_ASSET_CONTRACT}\",\"amount_atomic\":\"${AMOUNT}\",\"expires_in_secs\":3600}")
INTENT_ID=$(echo "${INTENT}" | python -c 'import json,sys; print(json.load(sys.stdin)["intent_id"])')
MEMO=$(echo "${INTENT}" | python -c 'import json,sys; print(json.load(sys.stdin)["memo"])')
DEPOSIT=$(echo "${INTENT}" | python -c 'import json,sys; print(json.load(sys.stdin)["deposit_address"])')
echo "intent:        ${INTENT_ID}"
echo "deposit to:    ${DEPOSIT}  (the gateway account)"
echo "asset:         ${NEAR_ASSET_CONTRACT}"
echo "amount:        ${AMOUNT} atomic units"
echo "memo REQUIRED: ${MEMO}  <- the intent id"

echo
echo "== 3. YOUR STEP: pay from your wallet (the only on-chain action) =="
echo "Send ft_transfer on the asset contract with:"
echo "  receiver_id = ${DEPOSIT}"
echo "  amount      = ${AMOUNT}"
echo "  memo        = ${MEMO}"
echo "  attached deposit = exactly 1 yoctoNEAR"
read -r -p "transaction hash after you sent it: " TX_HASH
read -r -p "signer account id (your wallet account): " SIGNER

echo "== 4. verifying the deposit on chain =="
OBS=$(curl -sf -X POST "${BASE}/payment/intents/${INTENT_ID}/observations" \
  -H 'Content-Type: application/json' \
  -d "{\"signer_id\":\"${SIGNER}\",\"tx_hash\":\"${TX_HASH}\"}")
echo "${OBS}" | python -c 'import json,sys; o=json.load(sys.stdin); print("reconciled:", o["reconciled"], " funded:", o["funded"])'
echo "${OBS}" | python -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("funded") else 1)' || {
  echo "not funded - observe again with the same body once the tx is final."; exit 1; }

echo "== 5. reserving and running the paid scan =="
KEY="demo-$(head -c 4 /dev/urandom | od -An -tx1 | tr -d ' \n')"
RES=$(curl -sf -X POST "${BASE}/reservations" -H 'Content-Type: application/json' \
  -d "{\"account_id\":\"${ACCOUNT}\",\"sku\":\"scan.v1\",\"amount_atomic\":\"${AMOUNT}\",\"idempotency_key\":\"${KEY}\"}")
JOB=$(echo "${RES}" | python -c 'import json,sys; print(json.load(sys.stdin)["job_id"])')
curl -sf -X POST "${BASE}/jobs/${JOB}/start" -H 'Content-Type: application/json' -d '{}' >/dev/null
SCAN=$(curl -s -X POST "${BASE}/jobs/${JOB}/scan" -H 'Content-Type: application/json' --data-binary @demo/envelope.json)
echo "${SCAN}" | python -c 'import json,sys; s=json.load(sys.stdin); r=s["report"]; print("decision:", r["decision"], " confidence:", r["confidence_band"]); print("charged:", s["settlement"]["charged"]["atomic"], "atomic units")'

echo "== 6. final balances =="
curl -sf "${BASE}/accounts/${ACCOUNT}" | python -m json.tool
echo "== near demo complete =="
