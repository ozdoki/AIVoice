Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$exePath = Join-Path $repoRoot "src-tauri\target\release\aivoice.exe"
$tauriCmd = Join-Path $repoRoot "node_modules\.bin\tauri.cmd"

if (-not (Test-Path $tauriCmd)) {
  throw "Tauri CLI was not found at $tauriCmd. Run dependency install first."
}

Get-Process -Name "aivoice" -ErrorAction SilentlyContinue |
  ForEach-Object {
    Write-Host "Stopping KoeType process $($_.Id): $($_.Path)"
    Stop-Process -Id $_.Id -Force
  }

Push-Location $repoRoot
try {
  & $tauriCmd build --no-bundle
  if ($LASTEXITCODE -ne 0) {
    throw "tauri build --no-bundle failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

if (-not (Test-Path $exePath)) {
  throw "Expected release executable was not created: $exePath"
}

Write-Host "Starting $exePath"
Start-Process -FilePath $exePath
