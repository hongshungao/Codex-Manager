use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;
use zip::ZipArchive;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const DEFAULT_UPDATE_REPO: &str = "qxcnm/Codex-Manager";
const PORTABLE_MARKER_FILE: &str = ".codexmanager-portable";
const CHECKSUMS_FILE: &str = "checksums.txt";
const PENDING_UPDATE_FILE: &str = "pending-update.json";
const USER_AGENT: &str = "CodexManager-Updater";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
  name: String,
  browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
  tag_name: String,
  name: Option<String>,
  published_at: Option<String>,
  draft: bool,
  prerelease: bool,
  assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
  repo: String,
  mode: String,
  is_portable: bool,
  has_update: bool,
  can_prepare: bool,
  current_version: String,
  latest_version: String,
  release_tag: String,
  release_name: Option<String>,
  published_at: Option<String>,
  reason: Option<String>,
  checked_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrepareResponse {
  prepared: bool,
  mode: String,
  is_portable: bool,
  release_tag: String,
  latest_version: String,
  asset_name: String,
  asset_path: String,
  downloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateActionResponse {
  ok: bool,
  message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdate {
  mode: String,
  is_portable: bool,
  release_tag: String,
  latest_version: String,
  asset_name: String,
  asset_path: String,
  installer_path: Option<String>,
  staging_dir: Option<String>,
  prepared_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusResponse {
  repo: String,
  mode: String,
  is_portable: bool,
  current_version: String,
  current_exe_path: String,
  portable_marker_path: String,
  pending: Option<PendingUpdate>,
  last_check: Option<UpdateCheckResponse>,
  last_error: Option<String>,
}

#[derive(Debug, Default)]
struct UpdaterState {
  last_check: Option<UpdateCheckResponse>,
  last_error: Option<String>,
}

struct ResolvedUpdateContext {
  check: UpdateCheckResponse,
  payload_asset: Option<GitHubAsset>,
  checksums_asset: Option<GitHubAsset>,
}

static UPDATER_STATE: OnceLock<Mutex<UpdaterState>> = OnceLock::new();

fn updater_state() -> &'static Mutex<UpdaterState> {
  UPDATER_STATE.get_or_init(|| Mutex::new(UpdaterState::default()))
}

fn now_unix_secs() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|v| v.as_secs())
    .unwrap_or(0)
}

fn resolve_update_repo() -> String {
  std::env::var("CODEXMANAGER_UPDATE_REPO")
    .ok()
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| DEFAULT_UPDATE_REPO.to_string())
}

fn normalize_version(input: &str) -> Result<Version, String> {
  let normalized = input.trim().trim_start_matches(['v', 'V']);
  Version::parse(normalized).map_err(|err| format!("invalid version '{input}': {err}"))
}

fn current_exe_path() -> Result<PathBuf, String> {
  std::env::current_exe().map_err(|err| format!("resolve current exe failed: {err}"))
}

fn current_mode_and_marker() -> Result<(String, bool, PathBuf, PathBuf), String> {
  let exe = current_exe_path()?;
  let exe_dir = exe
    .parent()
    .ok_or_else(|| "resolve exe parent dir failed".to_string())?
    .to_path_buf();
  let marker = exe_dir.join(PORTABLE_MARKER_FILE);
  let is_portable = marker.is_file();
  let mode = if is_portable { "portable" } else { "installer" }.to_string();
  Ok((mode, is_portable, exe, marker))
}

fn http_client() -> Result<Client, String> {
  Client::builder()
    .timeout(Duration::from_secs(30))
    .build()
    .map_err(|err| format!("build http client failed: {err}"))
}

fn fetch_latest_release(client: &Client, repo: &str) -> Result<GitHubRelease, String> {
  if !repo.contains('/') {
    return Err(format!(
      "invalid update repo '{repo}', expected owner/repo format"
    ));
  }
  let url = format!("https://api.github.com/repos/{repo}/releases/latest");
  let release = client
    .get(url)
    .header(reqwest::header::USER_AGENT, USER_AGENT)
    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
    .send()
    .map_err(|err| format!("request latest release failed: {err}"))?
    .error_for_status()
    .map_err(|err| format!("latest release response not successful: {err}"))?
    .json::<GitHubRelease>()
    .map_err(|err| format!("parse latest release payload failed: {err}"))?;

  if release.draft || release.prerelease {
    return Err("latest release must be a stable release".to_string());
  }
  Ok(release)
}

fn select_checksum_asset(assets: &[GitHubAsset]) -> Option<GitHubAsset> {
  assets
    .iter()
    .find(|asset| asset.name.eq_ignore_ascii_case(CHECKSUMS_FILE))
    .cloned()
}

fn portable_asset_name_for_platform() -> &'static str {
  if cfg!(target_os = "windows") {
    "CodexManager-windows-portable.zip"
  } else if cfg!(target_os = "macos") {
    "CodexManager-macos-portable.zip"
  } else {
    "CodexManager-linux-portable.zip"
  }
}

fn select_payload_asset(mode: &str, assets: &[GitHubAsset]) -> Option<GitHubAsset> {
  if mode == "portable" {
    let portable_name = portable_asset_name_for_platform();
    return assets
      .iter()
      .find(|asset| asset.name.eq_ignore_ascii_case(portable_name))
      .cloned();
  }

  if cfg!(target_os = "windows") {
    if let Some(exe) = assets.iter().find(|asset| {
      let name = asset.name.to_ascii_lowercase();
      name.ends_with(".exe") && !name.contains("portable")
    }) {
      return Some(exe.clone());
    }
    return assets
      .iter()
      .find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name.ends_with(".msi") && !name.contains("portable")
      })
      .cloned();
  }

  if cfg!(target_os = "macos") {
    return assets
      .iter()
      .find(|asset| asset.name.to_ascii_lowercase().ends_with(".dmg"))
      .cloned();
  }

  if let Some(appimage) = assets
    .iter()
    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".appimage"))
  {
    return Some(appimage.clone());
  }
  if let Some(deb) = assets
    .iter()
    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".deb"))
  {
    return Some(deb.clone());
  }
  assets
    .iter()
    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".rpm"))
    .cloned()
}

fn resolve_update_context() -> Result<ResolvedUpdateContext, String> {
  let repo = resolve_update_repo();
  let (mode, is_portable, _, _) = current_mode_and_marker()?;
  let current_version = env!("CARGO_PKG_VERSION").to_string();
  let current_semver = normalize_version(&current_version)?;

  let client = http_client()?;
  let release = fetch_latest_release(&client, &repo)?;
  let latest_semver = normalize_version(&release.tag_name)?;
  let has_update = latest_semver > current_semver;

  let payload_asset = select_payload_asset(&mode, &release.assets);
  let checksums_asset = select_checksum_asset(&release.assets);
  let can_prepare = has_update && payload_asset.is_some() && checksums_asset.is_some();

  let reason = if !has_update {
    Some("current version is already up to date".to_string())
  } else if payload_asset.is_none() {
    Some("release asset for current platform/mode not found".to_string())
  } else if checksums_asset.is_none() {
    Some("checksums.txt is missing in release assets".to_string())
  } else {
    None
  };

  let check = UpdateCheckResponse {
    repo,
    mode,
    is_portable,
    has_update,
    can_prepare,
    current_version,
    latest_version: latest_semver.to_string(),
    release_tag: release.tag_name.clone(),
    release_name: release.name.clone(),
    published_at: release.published_at.clone(),
    reason,
    checked_at_unix_secs: now_unix_secs(),
  };

  Ok(ResolvedUpdateContext {
    check,
    payload_asset,
    checksums_asset,
  })
}

fn set_last_check(check: UpdateCheckResponse) {
  if let Ok(mut guard) = updater_state().lock() {
    guard.last_check = Some(check);
    guard.last_error = None;
  }
}

fn set_last_error(message: String) {
  if let Ok(mut guard) = updater_state().lock() {
    guard.last_error = Some(message);
  }
}

fn updates_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
  let mut root = app
    .path()
    .app_data_dir()
    .map_err(|_| "app data dir not found".to_string())?;
  root.push("updates");
  fs::create_dir_all(&root).map_err(|err| format!("create updates dir failed: {err}"))?;
  Ok(root)
}

fn pending_update_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
  Ok(updates_root_dir(app)?.join(PENDING_UPDATE_FILE))
}

fn read_pending_update(app: &tauri::AppHandle) -> Result<Option<PendingUpdate>, String> {
  let path = pending_update_path(app)?;
  if !path.is_file() {
    return Ok(None);
  }
  let bytes = fs::read(&path).map_err(|err| format!("read pending update failed: {err}"))?;
  let parsed = serde_json::from_slice::<PendingUpdate>(&bytes)
    .map_err(|err| format!("parse pending update failed: {err}"))?;
  Ok(Some(parsed))
}

fn write_pending_update(app: &tauri::AppHandle, pending: &PendingUpdate) -> Result<(), String> {
  let path = pending_update_path(app)?;
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|err| format!("create pending dir failed: {err}"))?;
  }
  let bytes = serde_json::to_vec_pretty(pending)
    .map_err(|err| format!("serialize pending update failed: {err}"))?;
  fs::write(&path, bytes).map_err(|err| format!("write pending update failed: {err}"))
}

fn clear_pending_update(app: &tauri::AppHandle) -> Result<(), String> {
  let path = pending_update_path(app)?;
  if path.exists() {
    fs::remove_file(&path).map_err(|err| format!("remove pending update failed: {err}"))?;
  }
  Ok(())
}

fn sanitize_tag(tag: &str) -> String {
  let out: String = tag
    .chars()
    .map(|ch| {
      if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
        ch
      } else {
        '_'
      }
    })
    .collect();
  if out.is_empty() {
    "unknown".to_string()
  } else {
    out
  }
}

fn download_to_file(client: &Client, url: &str, target: &Path) -> Result<(), String> {
  if let Some(parent) = target.parent() {
    fs::create_dir_all(parent).map_err(|err| format!("create download dir failed: {err}"))?;
  }
  let mut resp = client
    .get(url)
    .header(reqwest::header::USER_AGENT, USER_AGENT)
    .send()
    .map_err(|err| format!("download request failed: {err}"))?
    .error_for_status()
    .map_err(|err| format!("download response failed: {err}"))?;

  let mut file = File::create(target).map_err(|err| format!("create file failed: {err}"))?;
  std::io::copy(&mut resp, &mut file).map_err(|err| format!("write file failed: {err}"))?;
  file.flush().map_err(|err| format!("flush file failed: {err}"))
}

fn parse_checksums(contents: &str) -> HashMap<String, String> {
  let mut out = HashMap::new();
  for raw_line in contents.lines() {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    let mut parts = line.split_whitespace();
    let Some(hash) = parts.next() else {
      continue;
    };
    let Some(name) = parts.next() else {
      continue;
    };
    out.insert(name.trim_start_matches('*').to_string(), hash.to_ascii_lowercase());
  }
  out
}

fn sha256_for_file(path: &Path) -> Result<String, String> {
  let mut file = File::open(path).map_err(|err| format!("open file for hash failed: {err}"))?;
  let mut hasher = Sha256::new();
  let mut buf = [0u8; 8192];
  loop {
    let read = file
      .read(&mut buf)
      .map_err(|err| format!("read file for hash failed: {err}"))?;
    if read == 0 {
      break;
    }
    hasher.update(&buf[..read]);
  }
  Ok(format!("{:x}", hasher.finalize()))
}

fn find_expected_checksum<'a>(checksums: &'a HashMap<String, String>, asset_name: &str) -> Option<&'a String> {
  if let Some(v) = checksums.get(asset_name) {
    return Some(v);
  }
  checksums.iter().find_map(|(name, hash)| {
    let file_name = Path::new(name).file_name().and_then(|v| v.to_str())?;
    if file_name.eq_ignore_ascii_case(asset_name) {
      Some(hash)
    } else {
      None
    }
  })
}

fn verify_asset_checksum(
  asset_name: &str,
  asset_path: &Path,
  checksums_path: &Path,
) -> Result<(), String> {
  let checksums_raw = fs::read_to_string(checksums_path)
    .map_err(|err| format!("read checksums file failed: {err}"))?;
  let checksums = parse_checksums(&checksums_raw);
  let expected = find_expected_checksum(&checksums, asset_name)
    .ok_or_else(|| format!("checksum entry for asset '{asset_name}' not found"))?;
  let actual = sha256_for_file(asset_path)?;
  if actual.eq_ignore_ascii_case(expected) {
    Ok(())
  } else {
    Err(format!(
      "checksum mismatch for {asset_name}: expected {expected}, got {actual}"
    ))
  }
}

fn extract_zip_archive(zip_path: &Path, target_dir: &Path) -> Result<(), String> {
  let file = File::open(zip_path).map_err(|err| format!("open zip failed: {err}"))?;
  let mut archive = ZipArchive::new(file).map_err(|err| format!("read zip failed: {err}"))?;

  for idx in 0..archive.len() {
    let mut entry = archive
      .by_index(idx)
      .map_err(|err| format!("read zip entry failed: {err}"))?;
    let Some(relative_path) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
      continue;
    };
    let out_path = target_dir.join(relative_path);
    if entry.is_dir() {
      fs::create_dir_all(&out_path).map_err(|err| format!("create dir failed: {err}"))?;
      continue;
    }

    if let Some(parent) = out_path.parent() {
      fs::create_dir_all(parent).map_err(|err| format!("create parent dir failed: {err}"))?;
    }
    let mut out_file = File::create(&out_path).map_err(|err| format!("create file failed: {err}"))?;
    std::io::copy(&mut entry, &mut out_file).map_err(|err| format!("extract file failed: {err}"))?;

    #[cfg(unix)]
    if let Some(mode) = entry.unix_mode() {
      let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
    }
  }

  Ok(())
}

fn prepare_update_impl(app: &tauri::AppHandle) -> Result<UpdatePrepareResponse, String> {
  let context = resolve_update_context()?;
  set_last_check(context.check.clone());

  if !context.check.has_update {
    return Err("current version is already up to date".to_string());
  }
  if !context.check.can_prepare {
    return Err(context
      .check
      .reason
      .clone()
      .unwrap_or_else(|| "update is not ready to prepare".to_string()));
  }

  let payload_asset = context
    .payload_asset
    .clone()
    .ok_or_else(|| "missing payload asset".to_string())?;
  let checksums_asset = context
    .checksums_asset
    .clone()
    .ok_or_else(|| "missing checksums asset".to_string())?;

  let client = http_client()?;
  let release_dir = updates_root_dir(app)?.join(sanitize_tag(&context.check.release_tag));
  fs::create_dir_all(&release_dir).map_err(|err| format!("create release dir failed: {err}"))?;

  let payload_path = release_dir.join(&payload_asset.name);
  let checksums_path = release_dir.join(CHECKSUMS_FILE);
  download_to_file(&client, &payload_asset.browser_download_url, &payload_path)?;
  download_to_file(&client, &checksums_asset.browser_download_url, &checksums_path)?;
  verify_asset_checksum(&payload_asset.name, &payload_path, &checksums_path)?;

  let mut pending = PendingUpdate {
    mode: context.check.mode.clone(),
    is_portable: context.check.is_portable,
    release_tag: context.check.release_tag.clone(),
    latest_version: context.check.latest_version.clone(),
    asset_name: payload_asset.name.clone(),
    asset_path: payload_path.display().to_string(),
    installer_path: None,
    staging_dir: None,
    prepared_at_unix_secs: now_unix_secs(),
  };

  if context.check.mode == "portable" {
    let staging_dir = release_dir.join("staging");
    if staging_dir.is_dir() {
      fs::remove_dir_all(&staging_dir).map_err(|err| format!("clean staging dir failed: {err}"))?;
    }
    fs::create_dir_all(&staging_dir).map_err(|err| format!("create staging dir failed: {err}"))?;
    extract_zip_archive(&payload_path, &staging_dir)?;
    let marker = staging_dir.join(PORTABLE_MARKER_FILE);
    if !marker.is_file() {
      return Err(format!(
        "portable package is invalid: missing marker file {PORTABLE_MARKER_FILE}"
      ));
    }
    pending.staging_dir = Some(staging_dir.display().to_string());
  } else {
    pending.installer_path = Some(payload_path.display().to_string());
  }

  write_pending_update(app, &pending)?;

  Ok(UpdatePrepareResponse {
    prepared: true,
    mode: context.check.mode,
    is_portable: context.check.is_portable,
    release_tag: context.check.release_tag,
    latest_version: context.check.latest_version,
    asset_name: pending.asset_name,
    asset_path: pending.asset_path,
    downloaded: true,
  })
}

fn script_dir_from_pending(pending: &PendingUpdate, app: &tauri::AppHandle) -> Result<PathBuf, String> {
  let asset_path = PathBuf::from(&pending.asset_path);
  if let Some(parent) = asset_path.parent() {
    return Ok(parent.to_path_buf());
  }
  updates_root_dir(app)
}

fn spawn_portable_apply_worker(
  script_dir: &Path,
  target_dir: &Path,
  staging_dir: &Path,
  exe_name: &str,
  pending_path: &Path,
  pid_to_wait: u32,
) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    let script_path = script_dir.join("apply-portable-update.ps1");
    let script = r#"
param(
  [Parameter(Mandatory=$true)][string]$TargetDir,
  [Parameter(Mandatory=$true)][string]$StagingDir,
  [Parameter(Mandatory=$true)][string]$ExeName,
  [Parameter(Mandatory=$true)][string]$PendingFile,
  [Parameter(Mandatory=$true)][int]$PidToWait
)
$ErrorActionPreference = "Stop"
for ($i = 0; $i -lt 240; $i++) {
  if (-not (Get-Process -Id $PidToWait -ErrorAction SilentlyContinue)) { break }
  Start-Sleep -Milliseconds 500
}
Get-ChildItem -LiteralPath $StagingDir -Force | ForEach-Object {
  Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $TargetDir $_.Name) -Recurse -Force
}
if (Test-Path -LiteralPath $PendingFile) {
  Remove-Item -LiteralPath $PendingFile -Force -ErrorAction SilentlyContinue
}
Start-Process -FilePath (Join-Path $TargetDir $ExeName) | Out-Null
"#;
    fs::write(&script_path, script).map_err(|err| format!("write apply script failed: {err}"))?;

    let args = vec![
      "-TargetDir".to_string(),
      target_dir.display().to_string(),
      "-StagingDir".to_string(),
      staging_dir.display().to_string(),
      "-ExeName".to_string(),
      exe_name.to_string(),
      "-PendingFile".to_string(),
      pending_path.display().to_string(),
      "-PidToWait".to_string(),
      pid_to_wait.to_string(),
    ];

    let try_spawn = |shell: &str| -> Result<(), String> {
      let mut cmd = Command::new(shell);
      cmd
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .args(&args);
      cmd.creation_flags(CREATE_NO_WINDOW);
      cmd
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("spawn {shell} failed: {err}"))
    };

    if try_spawn("powershell.exe").is_ok() {
      return Ok(());
    }
    return try_spawn("pwsh.exe");
  }

  #[cfg(not(target_os = "windows"))]
  {
    let script_path = script_dir.join("apply-portable-update.sh");
    let script = r#"#!/usr/bin/env sh
TARGET_DIR="$1"
STAGING_DIR="$2"
EXE_NAME="$3"
PENDING_FILE="$4"
PID_TO_WAIT="$5"

i=0
while kill -0 "$PID_TO_WAIT" 2>/dev/null && [ "$i" -lt 240 ]; do
  i=$((i + 1))
  sleep 0.5
done

cp -Rf "$STAGING_DIR"/. "$TARGET_DIR"/
rm -f "$PENDING_FILE"
chmod +x "$TARGET_DIR/$EXE_NAME" 2>/dev/null || true
"$TARGET_DIR/$EXE_NAME" >/dev/null 2>&1 &
"#;
    fs::write(&script_path, script).map_err(|err| format!("write apply script failed: {err}"))?;

    #[cfg(unix)]
    {
      fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
        .map_err(|err| format!("chmod apply script failed: {err}"))?;
    }

    Command::new("sh")
      .arg(&script_path)
      .arg(target_dir)
      .arg(staging_dir)
      .arg(exe_name)
      .arg(pending_path)
      .arg(pid_to_wait.to_string())
      .spawn()
      .map_err(|err| format!("spawn apply script failed: {err}"))?;
    Ok(())
  }
}

fn schedule_app_exit(app: tauri::AppHandle) {
  std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(280));
    app.exit(0);
  });
}

fn launch_installer(installer_path: &Path) -> Result<(), String> {
  if !installer_path.is_file() {
    return Err(format!("installer not found: {}", installer_path.display()));
  }

  #[cfg(target_os = "windows")]
  {
    let mut cmd = Command::new(installer_path);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn()
      .map_err(|err| format!("launch installer failed: {err}"))?;
    return Ok(());
  }

  #[cfg(target_os = "macos")]
  {
    Command::new("open")
      .arg(installer_path)
      .spawn()
      .map_err(|err| format!("open installer failed: {err}"))?;
    return Ok(());
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  {
    let ext = installer_path
      .extension()
      .and_then(|v| v.to_str())
      .unwrap_or("")
      .to_ascii_lowercase();

    if ext == "appimage" {
      #[cfg(unix)]
      {
        let _ = fs::set_permissions(installer_path, fs::Permissions::from_mode(0o755));
      }
      Command::new(installer_path)
        .spawn()
        .map_err(|err| format!("launch AppImage failed: {err}"))?;
      return Ok(());
    }

    Command::new("xdg-open")
      .arg(installer_path)
      .spawn()
      .map_err(|err| format!("open installer failed: {err}"))?;
    Ok(())
  }
}

#[tauri::command]
pub async fn app_update_check() -> Result<UpdateCheckResponse, String> {
  let task = tauri::async_runtime::spawn_blocking(resolve_update_context);
  match task.await {
    Ok(Ok(context)) => {
      set_last_check(context.check.clone());
      Ok(context.check)
    }
    Ok(Err(err)) => {
      set_last_error(err.clone());
      Err(err)
    }
    Err(err) => {
      let message = format!("app_update_check task failed: {err}");
      set_last_error(message.clone());
      Err(message)
    }
  }
}

#[tauri::command]
pub async fn app_update_prepare(app: tauri::AppHandle) -> Result<UpdatePrepareResponse, String> {
  let app_handle = app.clone();
  let task = tauri::async_runtime::spawn_blocking(move || prepare_update_impl(&app_handle));
  match task.await {
    Ok(Ok(result)) => {
      if let Ok(mut guard) = updater_state().lock() {
        guard.last_error = None;
      }
      Ok(result)
    }
    Ok(Err(err)) => {
      set_last_error(err.clone());
      Err(err)
    }
    Err(err) => {
      let message = format!("app_update_prepare task failed: {err}");
      set_last_error(message.clone());
      Err(message)
    }
  }
}

#[tauri::command]
pub fn app_update_apply_portable(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
  let pending = read_pending_update(&app)?
    .ok_or_else(|| "no prepared update found, call app_update_prepare first".to_string())?;

  if pending.mode != "portable" {
    return Err("prepared update is not portable mode".to_string());
  }

  let staging_dir = PathBuf::from(
    pending
      .staging_dir
      .as_ref()
      .ok_or_else(|| "portable update staging dir is missing".to_string())?,
  );
  if !staging_dir.is_dir() {
    return Err(format!("staging dir not found: {}", staging_dir.display()));
  }

  let exe_path = current_exe_path()?;
  let target_dir = exe_path
    .parent()
    .ok_or_else(|| "resolve target app dir failed".to_string())?
    .to_path_buf();
  let exe_name = exe_path
    .file_name()
    .and_then(|name| name.to_str())
    .ok_or_else(|| "resolve current exe file name failed".to_string())?
    .to_string();
  let pending_path = pending_update_path(&app)?;
  let script_dir = script_dir_from_pending(&pending, &app)?;
  let pid = std::process::id();

  spawn_portable_apply_worker(
    &script_dir,
    &target_dir,
    &staging_dir,
    &exe_name,
    &pending_path,
    pid,
  )?;

  schedule_app_exit(app);
  Ok(UpdateActionResponse {
    ok: true,
    message: "portable update prepared, app will restart to finish replacement".to_string(),
  })
}

#[tauri::command]
pub fn app_update_launch_installer(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
  let pending = read_pending_update(&app)?
    .ok_or_else(|| "no prepared update found, call app_update_prepare first".to_string())?;
  if pending.mode != "installer" {
    return Err("prepared update is not installer mode".to_string());
  }

  let installer_path = PathBuf::from(
    pending
      .installer_path
      .as_ref()
      .ok_or_else(|| "installer path is missing in pending update".to_string())?,
  );

  launch_installer(&installer_path)?;
  clear_pending_update(&app)?;

  Ok(UpdateActionResponse {
    ok: true,
    message: format!("installer launched: {}", installer_path.display()),
  })
}

#[tauri::command]
pub fn app_update_status(app: tauri::AppHandle) -> Result<UpdateStatusResponse, String> {
  let repo = resolve_update_repo();
  let (mode, is_portable, exe_path, marker_path) = current_mode_and_marker()?;
  let pending = read_pending_update(&app)?;
  let (last_check, last_error) = if let Ok(guard) = updater_state().lock() {
    (guard.last_check.clone(), guard.last_error.clone())
  } else {
    (
      None,
      Some("failed to read updater state lock".to_string()),
    )
  };

  Ok(UpdateStatusResponse {
    repo,
    mode,
    is_portable,
    current_version: env!("CARGO_PKG_VERSION").to_string(),
    current_exe_path: exe_path.display().to_string(),
    portable_marker_path: marker_path.display().to_string(),
    pending,
    last_check,
    last_error,
  })
}
