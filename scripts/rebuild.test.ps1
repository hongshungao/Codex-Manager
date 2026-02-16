$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "rebuild.ps1"
if (-not (Test-Path $scriptPath)) {
  throw "missing rebuild.ps1 at $scriptPath"
}

$output = & $scriptPath -DryRun -Bundle nsis -CleanDist -Portable 2>&1 | Out-String
if (-not $?) {
  throw "rebuild.ps1 failed to run"
}
if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) {
  throw "rebuild.ps1 exited with code $LASTEXITCODE"
}

if ($output -notmatch "DRY RUN: remove" -and $output -notmatch "skip:") {
  throw "expected cleanup output"
}
if ($output -notlike '*src-tauri\target*') {
  throw "expected src-tauri target cleanup in output"
}
if ($output -notlike "*cargo tauri build --bundles nsis*") {
  throw "expected tauri build command in output"
}
if ($output -notmatch "portable") {
  throw "expected portable output in dry-run"
}

Write-Host "rebuild.ps1 dry-run output looks ok"

$multiOutput = & $scriptPath -DryRun -AllPlatforms -GitRef "master" -GithubToken "dummy" 2>&1 | Out-String
if (-not $?) {
  throw "rebuild.ps1 -AllPlatforms dry-run failed to run"
}
if ($multiOutput -notlike "*dispatch workflow release-multi-platform.yml*") {
  throw "expected all-platform dispatch output"
}
if ($multiOutput -notlike "*repos/*/actions/workflows/release-multi-platform.yml/dispatches*") {
  throw "expected github dispatch url in dry-run output"
}

Write-Host "rebuild.ps1 all-platform dry-run output looks ok"
