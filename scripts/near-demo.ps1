# Operator-gated mainnet E2E for the NEAR payment adapter.
#
# This script performs ONE on-chain action, and it is yours, not the
# script's: the deposit transfer happens in YOUR wallet when you approve
# it. The script itself only talks to the local API and verifies the
# result on chain through the adapter. See docs/integrations/near.md.
#
# Required environment (see .env.example):
#   APS_PROVIDER=near, NEAR_NETWORK, NEAR_GATEWAY_ACCOUNT, NEAR_ASSET_CONTRACT
#   (optional: NEAR_RPC_URL, APS_PORT)
# Optional: ACCOUNT (ledger account id, default near-demo), AMOUNT_ATOMIC
#   (default 1000000 = 1 USDC in 6-decimal atomic units).

param(
    [string]$Account = "near-demo",
    [string]$AmountAtomic = "1000000"
)

$ErrorActionPreference = "Stop"
$Port = if ($env:APS_PORT) { $env:APS_PORT } else { "8080" }
$Base = "http://127.0.0.1:$Port"

if (-not $env:APS_PROVIDER -or $env:APS_PROVIDER -ne "near") {
    Write-Host "APS_PROVIDER must be 'near' (with NEAR_NETWORK, NEAR_GATEWAY_ACCOUNT,"
    Write-Host "NEAR_ASSET_CONTRACT set) - see .env.example and docs/integrations/near.md."
    exit 2
}

Write-Host "== 1. starting aps-api with the near provider =="
$server = Start-Process -FilePath "cargo" -ArgumentList "run", "-q", "-p", "aps-api" -WorkingDirectory (Get-Location) -PassThru -NoNewWindow -RedirectStandardError "target/near-demo-server.log"
try {
    $healthy = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        try {
            $null = Invoke-RestMethod -Uri "$Base/health" -Method Get -TimeoutSec 2
            $healthy = $true; break
        } catch { }
    }
    if (-not $healthy) { throw "aps-api did not become healthy; see target/near-demo-server.log" }

    Write-Host "== 2. creating the deposit intent =="
    $intentBody = @{
        account_id      = $Account
        asset_id        = $env:NEAR_ASSET_CONTRACT
        amount_atomic   = $AmountAtomic
        expires_in_secs = 3600
    } | ConvertTo-Json
    $intent = Invoke-RestMethod -Uri "$Base/payment/intents" -Method Post -ContentType "application/json" -Body $intentBody
    $intentId = $intent.intent_id
    Write-Host "intent:        $intentId"
    Write-Host "deposit to:    $($intent.deposit_address)  (the gateway account)"
    Write-Host "asset:         $($intent.asset_id)"
    Write-Host "amount:        $($intent.amount.atomic) atomic units ($($intent.amount.decimal))"
    Write-Host "memo REQUIRED: $($intent.memo)  <- the intent id"

    Write-Host ""
    Write-Host "== 3. YOUR STEP: pay from your wallet (the only on-chain action) =="
    Write-Host "Send ft_transfer on the asset contract with:"
    Write-Host "  receiver_id = $($intent.deposit_address)"
    Write-Host "  amount      = $($intent.amount.atomic)"
    Write-Host "  memo        = $($intent.memo)"
    Write-Host "  attached deposit = exactly 1 yoctoNEAR"
    $jsonArgs = '{"amount":"' + $intent.amount.atomic + '","receiver_id":"' + $intent.deposit_address + '","memo":"' + $intent.memo + '"}'
    Write-Host "(near-cli-rs shape: near contract call-function <asset-contract> ft_transfer json-args '$jsonArgs' --attached-deposit 1yoctoNEAR --gas 30Tgas as <your-account> network-config <network> sign-with-keychain send)"
    $txHash = Read-Host "transaction hash after you sent it"
    $signer = Read-Host "signer account id (your wallet account)"

    Write-Host "== 4. verifying the deposit on chain =="
    $hint = @{ signer_id = $signer; tx_hash = $txHash } | ConvertTo-Json
    $observation = Invoke-RestMethod -Uri "$Base/payment/intents/$intentId/observations" -Method Post -ContentType "application/json" -Body $hint
    Write-Host "reconciled: $($observation.reconciled)  funded: $($observation.funded)"
    if (-not $observation.funded) {
        Write-Host "not funded - observe again with the same body once the tx is final,"
        Write-Host "or inspect the mismatch details above."
        exit 1
    }

    Write-Host "== 5. reserving and running the paid scan =="
    $key = "demo-$([guid]::NewGuid().ToString('N').Substring(0,8))"
    $reservation = Invoke-RestMethod -Uri "$Base/reservations" -Method Post -ContentType "application/json" -Body (@{
        account_id      = $Account
        sku             = "scan.v1"
        amount_atomic   = $AmountAtomic
        idempotency_key = $key } | ConvertTo-Json)
    $job = $reservation.job_id
    $null = Invoke-RestMethod -Uri "$Base/jobs/$job/start" -Method Post -ContentType "application/json" -Body "{}"
    $envelope = Get-Content "demo/envelope.json" -Raw
    $scan = Invoke-RestMethod -Uri "$Base/jobs/$job/scan" -Method Post -ContentType "application/json" -Body $envelope
    Write-Host "decision: $($scan.report.decision)  confidence: $($scan.report.confidence_band)"
    Write-Host "charged:  $($scan.settlement.charged.atomic) atomic units"
    Write-Host "content hash binds the settlement: $($scan.settlement.report_hash_hex.Substring(0,16))..."

    Write-Host "== 6. final balances =="
    $balances = Invoke-RestMethod -Uri "$Base/accounts/$Account" -Method Get
    $balances | ConvertTo-Json -Depth 5
    Write-Host "== near demo complete =="
} finally {
    if ($server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force
        Write-Host "(aps-api process $($server.Id) stopped)"
    }
}
