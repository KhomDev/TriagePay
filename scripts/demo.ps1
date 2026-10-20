# AgentPay Sentinel demo: prepaid scan, paid, executed, settled, refunded.
#
# Starts the API on an ephemeral port and walks the full product flow:
# provider-observed funding, reservation, scan, settlement against the
# report hash, an invalid-envelope rejection, and a refund.
#
# Requirements: cargo, python (3.10+) on PATH.

$ErrorActionPreference = "Stop"
$port = if ($env:APS_DEMO_PORT) { $env:APS_DEMO_PORT } else { (Get-Random -Minimum 8800 -Maximum 9900).ToString() }
$runKey = [string][DateTime]::UtcNow.Ticks
$base = "http://127.0.0.1:$port"
$repoRoot = Split-Path -Parent $PSScriptRoot

Write-Host "== AgentPay Sentinel demo =="

Write-Host "-- starting aps-api on port $port"
$env:APS_PORT = $port
$server = Start-Process -FilePath cargo -ArgumentList @("run", "-q", "-p", "aps-api", "--bin", "aps-api") -WorkingDirectory $repoRoot -PassThru -NoNewWindow -RedirectStandardError "$env:TEMP\aps-demo-server.log"
try {
  $up = $false
  for ($i = 0; $i -lt 60 -and -not $up; $i++) {
    Start-Sleep -Seconds 1
    try {
      $health = Invoke-RestMethod -Uri "$base/health" -TimeoutSec 2
      $up = $true
    } catch {}
  }
  if (-not $up) { throw "server did not come up" }
  Write-Host "-- server is up: $($health.status)"

  Write-Host "-- 1. create a payment intent: 5 USDC for account demo-agent"
  $intentBody = @{
    account_id = "demo-agent"
    asset_id = "asset.usdc.test"
    amount_atomic = "5000000"
    expires_in_secs = 3600
  } | ConvertTo-Json -Compress
  $intent = Invoke-RestMethod -Method Post -Uri "$base/payment/intents" -ContentType "application/json" -Body $intentBody
  $intent | ConvertTo-Json -Depth 10

  Write-Host "-- 2. observe the intent: the provider confirms, the account is funded"
  $obs = Invoke-RestMethod -Method Post -Uri "$base/payment/intents/$($intent.intent_id)/observations"
  $obs | ConvertTo-Json -Depth 10

  Write-Host "-- 3. reserve one scan (scan.v1 = 1 USDC) and start the job"
  $resBody = @{
    account_id = "demo-agent"
    sku = "scan.v1"
    amount_atomic = "1000000"
    idempotency_key = "demo-r1-$runKey"
  } | ConvertTo-Json -Compress
  $res = Invoke-RestMethod -Method Post -Uri "$base/reservations" -ContentType "application/json" -Body $resBody
  Write-Host "reserved job: $($res.job_id)"
  Invoke-RestMethod -Method Post -Uri "$base/jobs/$($res.job_id)/start" | ConvertTo-Json -Depth 10

  Write-Host "-- 4. scan a finding: the scanner runs, the report settles the job"
  $envelope = Get-Content -Raw "$repoRoot\demo\envelope.json"
  $scan = Invoke-RestMethod -Method Post -Uri "$base/jobs/$($res.job_id)/scan" -ContentType "application/json" -Body $envelope
  $scan | ConvertTo-Json -Depth 10

  Write-Host "-- 5. an invalid envelope is rejected with 400 and never charges"
  $bad = $envelope | ConvertFrom-Json
  $bad.title = "x" * 201
  $res2 = Invoke-RestMethod -Method Post -Uri "$base/reservations" -ContentType "application/json" -Body $resBody.Replace("demo-r1-$runKey", "demo-r2-$runKey")
  Invoke-RestMethod -Method Post -Uri "$base/jobs/$($res2.job_id)/start" | Out-Null
  try {
    Invoke-RestMethod -Method Post -Uri "$base/jobs/$($res2.job_id)/scan" -ContentType "application/json" -Body ($bad | ConvertTo-Json -Depth 5) | Out-Null
    Write-Host "ERROR: invalid envelope was accepted"
  } catch {
    $code = [int]$_.Exception.Response.StatusCode
    Write-Host "invalid envelope -> HTTP $code (expected 400)"
  }

  Write-Host "-- 6. refund the untouched reservation (simulated timeout)"
  $refundBody = @{ reason = "demo: simulated timeout" } | ConvertTo-Json -Compress
  Invoke-RestMethod -Method Post -Uri "$base/jobs/$($res2.job_id)/refunds" -ContentType "application/json" -Body $refundBody | ConvertTo-Json -Depth 10

  Write-Host "-- final balances:"
  Invoke-RestMethod -Uri "$base/accounts/demo-agent" | ConvertTo-Json -Depth 10
  Write-Host "== demo complete =="
} finally {
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
}

