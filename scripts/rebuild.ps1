[CmdletBinding()]
param(
  [ValidateSet("nsis", "msi")]
  [string]$Bundle = "nsis",
  [switch]$NoBundle,
  [switch]$CleanDist,
  [switch]$Portable,
  [string]$PortableDir,
  [switch]$AllPlatforms,
  [string]$GithubToken,
  [string]$WorkflowFile = "release-windows.yml",
  [string]$GitRef,
  [string]$ReleaseTag,
  [switch]$NoVerify,
  [bool]$DownloadArtifacts = $true,
  [string]$ArtifactsDir,
  [ValidateRange(5, 300)]
  [int]$PollIntervalSec = 10,
  [ValidateRange(1, 360)]
  [int]$TimeoutMin = 60,
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $scriptDir
$appsRoot = Join-Path $root "apps"
$tauriDir = Join-Path $appsRoot "src-tauri"
$rootTarget = Join-Path $root "target"
$tauriTarget = Join-Path $tauriDir "target"
$distDir = Join-Path $appsRoot "dist"
$tauriConfig = Join-Path $tauriDir "tauri.conf.json"

$appName = "CodexManager"
if (Test-Path $tauriConfig) {
  $appName = (Get-Content $tauriConfig -Raw | ConvertFrom-Json).productName
}

$portableRoot = if ($PortableDir) { $PortableDir } else { Join-Path $root "portable" }
$portableExe = Join-Path $portableRoot "$appName-portable.exe"
$legacyPortableExe = Join-Path $portableRoot "$appName.exe"
$legacyPortableMarker = Join-Path $portableRoot ".codexmanager-portable"
$appExe = Join-Path $tauriDir "target\\release\\$appName.exe"
$artifactsRoot = if ($ArtifactsDir) { $ArtifactsDir } else { Join-Path $root "artifacts" }

function Write-Step {
  param([string]$Message)
  Write-Output $Message
}

function Remove-Dir {
  param([string]$Path)
  if (-not (Test-Path $Path)) {
    Write-Step "skip: $Path not found"
    return
  }
  if ($DryRun) {
    Write-Step "DRY RUN: remove $Path"
    return
  }
  & cmd /c "rmdir /s /q `"$Path`""
  if ($LASTEXITCODE -ne 0) {
    throw "failed to remove $Path"
  }
}

function Run-Cargo {
  param([string]$CommandLine, [scriptblock]$Action)
  if ($DryRun) {
    Write-Step "DRY RUN: $CommandLine"
    return
  }
  & $Action
  if ($LASTEXITCODE -ne 0) {
    throw "command failed: $CommandLine"
  }
}

function Get-GitHubRepoInfo {
  $remote = (& git remote get-url origin 2>$null) -join ""
  if ([string]::IsNullOrWhiteSpace($remote)) {
    throw "git remote origin not found"
  }
  if ($remote -match "github\.com[:/](?<owner>[^/]+)/(?<repo>[^/.]+?)(?:\.git)?$") {
    return @{
      owner = $matches.owner
      repo = $matches.repo
    }
  }
  throw "origin is not a GitHub repository: $remote"
}

function Resolve-GitHubToken {
  if (-not [string]::IsNullOrWhiteSpace($GithubToken)) {
    return $GithubToken.Trim()
  }
  foreach ($name in @("GITHUB_TOKEN", "GH_TOKEN")) {
    $value = [Environment]::GetEnvironmentVariable($name)
    if (-not [string]::IsNullOrWhiteSpace($value)) {
      return $value.Trim()
    }
  }
  throw "GitHub token required for -AllPlatforms. Pass -GithubToken or set GITHUB_TOKEN."
}

function Invoke-GitHubApi {
  param(
    [ValidateSet("GET", "POST")]
    [string]$Method,
    [string]$Uri,
    [string]$Token,
    [object]$Body
  )
  $headers = @{
    Authorization          = "Bearer $Token"
    Accept                 = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"           = "codexmanager-rebuild-script"
  }
  if ($Method -eq "GET") {
    return Invoke-RestMethod -Method Get -Uri $Uri -Headers $headers
  }
  $json = if ($null -eq $Body) { $null } else { $Body | ConvertTo-Json -Depth 10 -Compress }
  return Invoke-RestMethod -Method Post -Uri $Uri -Headers $headers -ContentType "application/json" -Body $json
}

function Resolve-WorkflowDefinition {
  param(
    [hashtable]$Repo,
    [string]$Token,
    [string]$WorkflowFile
  )

  $workflowUri = "https://api.github.com/repos/$($Repo.owner)/$($Repo.repo)/actions/workflows/$WorkflowFile"
  try {
    return Invoke-GitHubApi -Method GET -Uri $workflowUri -Token $Token -Body $null
  } catch {
    $listUri = "https://api.github.com/repos/$($Repo.owner)/$($Repo.repo)/actions/workflows?per_page=100"
    $workflowList = @()
    try {
      $resp = Invoke-GitHubApi -Method GET -Uri $listUri -Token $Token -Body $null
      if ($null -ne $resp.workflows) {
        $workflowList = $resp.workflows | ForEach-Object { $_.path } | Sort-Object
      }
    } catch {
      # no-op; keep fallback message
    }

    if ($workflowList.Count -gt 0) {
      throw "workflow '$WorkflowFile' not found. Available workflows: $($workflowList -join ', ')"
    }
    throw "workflow '$WorkflowFile' not found."
  }
}

function Invoke-LocalWindowsBuild {
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found in PATH"
  }
  if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Warning "pnpm not found; tauri beforeBuildCommand may fail."
  }

  Push-Location $root
  try {
    Remove-Dir $rootTarget
    Remove-Dir $tauriTarget
    if ($CleanDist) {
      Remove-Dir $distDir
    }

    Push-Location $tauriDir
    try {
      if ($NoBundle) {
        Run-Cargo "cargo tauri build --no-bundle" { cargo tauri build --no-bundle }
      } else {
        Run-Cargo "cargo tauri build --bundles $Bundle" { cargo tauri build --bundles $Bundle }
      }
    } finally {
      Pop-Location
    }

    if ($Portable) {
      if ($DryRun) {
        Write-Step "DRY RUN: stage portable -> $portableRoot"
        Write-Step "DRY RUN: remove legacy portable exe -> $legacyPortableExe"
        Write-Step "DRY RUN: remove legacy marker -> $legacyPortableMarker"
        Write-Step "DRY RUN: copy $appExe -> $portableExe"
      } else {
        if (-not (Test-Path $portableRoot)) {
          New-Item -ItemType Directory -Force $portableRoot | Out-Null
        }
        if (-not (Test-Path $appExe)) {
          throw "missing app exe: $appExe"
        }
        if ((Test-Path $legacyPortableExe) -and ($legacyPortableExe -ne $portableExe)) {
          Remove-Item -Force $legacyPortableExe
        }
        if (Test-Path $legacyPortableMarker) {
          Remove-Item -Force $legacyPortableMarker
        }
        Copy-Item -Force $appExe $portableExe
      }
    }
  } finally {
    Pop-Location
  }
}

function Invoke-AllPlatformBuild {
  $repo = Get-GitHubRepoInfo
  $token = Resolve-GitHubToken
  if ([string]::IsNullOrWhiteSpace($GitRef)) {
    $GitRef = (& git branch --show-current 2>$null) -join ""
  }
  if ([string]::IsNullOrWhiteSpace($GitRef)) {
    $GitRef = (& git describe --tags --exact-match 2>$null) -join ""
  }
  if ([string]::IsNullOrWhiteSpace($GitRef)) {
    throw "cannot resolve git ref. Pass -GitRef explicitly."
  }

  if ([string]::IsNullOrWhiteSpace($ReleaseTag)) {
    $ReleaseTag = (& git describe --tags --exact-match 2>$null) -join ""
  }
  if ([string]::IsNullOrWhiteSpace($ReleaseTag)) {
    throw "release tag required for -AllPlatforms. Pass -ReleaseTag (e.g. v0.0.6)."
  }

  $resolvedSha = (& git rev-parse --verify "$GitRef^{commit}" 2>$null) -join ""
  if ([string]::IsNullOrWhiteSpace($resolvedSha)) {
    throw "cannot resolve git ref to commit: $GitRef"
  }

  $workflow = Resolve-WorkflowDefinition -Repo $repo -Token $token -WorkflowFile $WorkflowFile
  Write-Step "using workflow: $($workflow.path)"

  $dispatchUri = "https://api.github.com/repos/$($repo.owner)/$($repo.repo)/actions/workflows/$WorkflowFile/dispatches"
  $runsUri = "https://api.github.com/repos/$($repo.owner)/$($repo.repo)/actions/workflows/$WorkflowFile/runs?event=workflow_dispatch&per_page=50"
  $runVerifyInput = if ($NoVerify) { "false" } else { "true" }
  $dispatchBody = @{
    ref = $GitRef
    inputs = @{
      tag        = $ReleaseTag
      ref        = $GitRef
      run_verify = $runVerifyInput
    }
  }

  if ($DryRun) {
    Write-Step "DRY RUN: dispatch workflow $WorkflowFile on ref=$GitRef tag=$ReleaseTag run_verify=$runVerifyInput"
    Write-Step "DRY RUN: resolved target sha $resolvedSha"
    Write-Step "DRY RUN: POST $dispatchUri"
    Write-Step "DRY RUN: payload $($dispatchBody | ConvertTo-Json -Depth 10 -Compress)"
    if ($DownloadArtifacts) {
      Write-Step "DRY RUN: download artifacts -> $artifactsRoot"
    }
    return
  }

  Write-Step "dispatching workflow: $WorkflowFile (ref=$GitRef tag=$ReleaseTag run_verify=$runVerifyInput)"
  Invoke-GitHubApi -Method POST -Uri $dispatchUri -Token $token -Body $dispatchBody | Out-Null

  $deadline = (Get-Date).ToUniversalTime().AddMinutes($TimeoutMin)
  $dispatchedAt = (Get-Date).ToUniversalTime().AddSeconds(-5)
  $run = $null

  while ((Get-Date).ToUniversalTime() -lt $deadline) {
    Start-Sleep -Seconds $PollIntervalSec
    $runs = Invoke-GitHubApi -Method GET -Uri $runsUri -Token $token -Body $null
    if ($null -eq $runs.workflow_runs) {
      continue
    }
    $run = $runs.workflow_runs |
      Where-Object {
        [DateTime]::Parse($_.created_at).ToUniversalTime() -ge $dispatchedAt -and
        $_.head_sha -eq $resolvedSha
      } |
      Sort-Object { [DateTime]::Parse($_.created_at).ToUniversalTime() } -Descending |
      Select-Object -First 1
    if ($null -eq $run) {
      Write-Step "waiting workflow run: ref=$GitRef sha=$resolvedSha"
      continue
    }

    Write-Step ("workflow run: id={0} status={1} conclusion={2} head_sha={3}" -f $run.id, $run.status, $run.conclusion, $run.head_sha)
    if ($run.status -eq "completed") {
      break
    }
  }

  if ($null -eq $run) {
    throw "workflow run not found within timeout"
  }
  if ($run.status -ne "completed") {
    throw "workflow did not complete within timeout"
  }
  if ($run.conclusion -ne "success") {
    throw "workflow failed: conclusion=$($run.conclusion)"
  }

  if (-not $DownloadArtifacts) {
    Write-Step "workflow succeeded"
    return
  }

  if (-not (Test-Path $artifactsRoot)) {
    New-Item -ItemType Directory -Force $artifactsRoot | Out-Null
  }

  $artifactsUri = "https://api.github.com/repos/$($repo.owner)/$($repo.repo)/actions/runs/$($run.id)/artifacts?per_page=100"
  $artifactsResp = Invoke-GitHubApi -Method GET -Uri $artifactsUri -Token $token -Body $null
  if ($null -eq $artifactsResp.artifacts -or $artifactsResp.artifacts.Count -eq 0) {
    throw "workflow succeeded but no artifacts were found"
  }

  $headers = @{
    Authorization          = "Bearer $token"
    Accept                 = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"           = "codexmanager-rebuild-script"
  }

  foreach ($artifact in $artifactsResp.artifacts) {
    if ($artifact.expired -eq $true) {
      continue
    }
    $zipName = "{0}-{1}.zip" -f $artifact.name, $artifact.id
    $zipPath = Join-Path $artifactsRoot $zipName
    Write-Step "download artifact: $($artifact.name) -> $zipPath"
    Invoke-WebRequest -Uri $artifact.archive_download_url -Headers $headers -OutFile $zipPath | Out-Null
  }
}

Push-Location $root
try {
  if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "git not found in PATH"
  }

  if ($AllPlatforms) {
    Invoke-AllPlatformBuild
  } else {
    Invoke-LocalWindowsBuild
  }
} finally {
  Pop-Location
}

Write-Step "done"
