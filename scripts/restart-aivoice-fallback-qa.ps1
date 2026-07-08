Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$exePath = Join-Path $repoRoot "src-tauri\target\release\aivoice.exe"

if (-not (Test-Path $exePath)) {
  throw "Release executable was not found: $exePath. Run npm run app:restart first."
}

Get-Process -Name "aivoice" -ErrorAction SilentlyContinue |
  ForEach-Object {
    Write-Host "Stopping KoeType process $($_.Id): $($_.Path)"
    Stop-Process -Id $_.Id -Force
  }

$env:AIVOICE_FORCE_REALTIME_FAIL = "1"
Write-Host "Starting fallback QA build with AIVOICE_FORCE_REALTIME_FAIL=1"
Start-Process -FilePath $exePath
