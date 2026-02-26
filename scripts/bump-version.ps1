param(
  [Parameter(Mandatory = $true)]
  [string]$Version
)

$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z\.-]+)?$') {
  throw "invalid semver: $Version"
}

$root = Split-Path -Parent $PSScriptRoot

function Update-TextFileVersion {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Pattern,
    [Parameter(Mandatory = $true)][string]$Replacement
  )
  $raw = Get-Content $Path -Raw
  if (-not [regex]::IsMatch($raw, $Pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)) {
    throw "version pattern not found: $Path"
  }
  $next = [regex]::Replace($raw, $Pattern, $Replacement, [System.Text.RegularExpressions.RegexOptions]::Multiline)
  Set-Content $Path $next
}

$cargoWorkspace = Join-Path $root 'Cargo.toml'
Update-TextFileVersion -Path $cargoWorkspace -Pattern '^(version\s*=\s*")([^"]+)(")(\r?)$' -Replacement "`${1}$Version`$3$4"

$tauriCargo = Join-Path $root 'apps/src-tauri/Cargo.toml'
Update-TextFileVersion -Path $tauriCargo -Pattern '^(version\s*=\s*")([^"]+)(")(\r?)$' -Replacement "`${1}$Version`$3$4"

$tauriConfPath = Join-Path $root 'apps/src-tauri/tauri.conf.json'
$tauriConf = Get-Content $tauriConfPath -Raw | ConvertFrom-Json
$tauriConf.version = $Version
$tauriConf | ConvertTo-Json -Depth 100 | Set-Content $tauriConfPath

Write-Host "Version updated to $Version"
Write-Host "- Cargo workspace: $cargoWorkspace"
Write-Host "- Tauri Cargo: $tauriCargo"
Write-Host "- Tauri conf: $tauriConfPath"
Write-Host "Next: run cargo check commands to refresh lockfiles if needed."
