use codexmanager_core::rpc::types::JsonRpcRequest;
use codexmanager_core::storage::Storage;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::Manager;
use std::thread;

mod updater;

#[tauri::command]
async fn service_initialize(addr: Option<String>) -> Result<serde_json::Value, String> {
  let v = tauri::async_runtime::spawn_blocking(move || rpc_call("initialize", addr, None))
    .await
    .map_err(|err| format!("initialize task failed: {err}"))??;
  // 连接探测必须确认对端确实是 codexmanager-service，避免端口被其他服务占用时误判“已连接”。
  let server_name = v
    .get("result")
    .and_then(|r| r.get("server_name"))
    .and_then(|s| s.as_str())
    .unwrap_or("");
  if server_name != "codexmanager-service" {
    let hint = if server_name.is_empty() {
      "missing server_name"
    } else {
      server_name
    };
    return Err(format!("Port is in use or unexpected service responded ({hint})"));
  }
  Ok(v)
}

async fn rpc_call_in_background(
  method: &'static str,
  addr: Option<String>,
  params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
  let method_name = method.to_string();
  let method_for_task = method_name.clone();
  tauri::async_runtime::spawn_blocking(move || rpc_call(&method_for_task, addr, params))
    .await
    .map_err(|err| format!("{method_name} task failed: {err}"))?
}

#[tauri::command]
async fn service_start(app: tauri::AppHandle, addr: String) -> Result<(), String> {
  let addr = normalize_addr(&addr)?;
  tauri::async_runtime::spawn_blocking(move || {
    log::info!("service_start requested addr={}", addr);
    // 中文注释：保存地址与回调地址，按需启动 service
    std::env::set_var("CODEXMANAGER_SERVICE_ADDR", &addr);
    stop_service();
    spawn_service_with_addr(&app, &addr)
  })
  .await
  .map_err(|err| format!("service_start task failed: {err}"))?
}

#[tauri::command]
async fn service_stop() -> Result<(), String> {
  tauri::async_runtime::spawn_blocking(move || {
    // 中文注释：显式停止 service 进程
    stop_service();
    Ok(())
  })
  .await
  .map_err(|err| format!("service_stop task failed: {err}"))?
}

#[tauri::command]
async fn service_account_list(addr: Option<String>) -> Result<serde_json::Value, String> {
  rpc_call_in_background("account/list", addr, None).await
}

#[tauri::command]
async fn service_account_delete(
  addr: Option<String>,
  account_id: String,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "accountId": account_id });
  rpc_call_in_background("account/delete", addr, Some(params)).await
}

#[tauri::command]
async fn service_account_update(
  addr: Option<String>,
  account_id: String,
  sort: i64,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "accountId": account_id, "sort": sort });
  rpc_call_in_background("account/update", addr, Some(params)).await
}

#[tauri::command]
async fn service_account_import(
  addr: Option<String>,
  contents: Option<Vec<String>>,
  content: Option<String>,
) -> Result<serde_json::Value, String> {
  let mut payload_contents = contents.unwrap_or_default();
  if let Some(single) = content {
    if !single.trim().is_empty() {
      payload_contents.push(single);
    }
  }
  let params = serde_json::json!({ "contents": payload_contents });
  rpc_call_in_background("account/import", addr, Some(params)).await
}

#[tauri::command]
async fn local_account_delete(
  app: tauri::AppHandle,
  account_id: String,
) -> Result<serde_json::Value, String> {
  let db_path = resolve_db_path_with_legacy_migration(&app)?;
  tauri::async_runtime::spawn_blocking(move || {
    let mut storage = Storage::open(db_path).map_err(|e| e.to_string())?;
    storage
      .delete_account(&account_id)
      .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true }))
  })
  .await
  .map_err(|err| format!("local_account_delete task failed: {err}"))?
}

#[tauri::command]
async fn service_usage_read(
  addr: Option<String>,
  account_id: Option<String>,
) -> Result<serde_json::Value, String> {
  let params = account_id.map(|id| serde_json::json!({ "accountId": id }));
  rpc_call_in_background("account/usage/read", addr, params).await
}

#[tauri::command]
async fn service_usage_list(addr: Option<String>) -> Result<serde_json::Value, String> {
  rpc_call_in_background("account/usage/list", addr, None).await
}

#[tauri::command]
async fn service_usage_refresh(
  addr: Option<String>,
  account_id: Option<String>,
) -> Result<serde_json::Value, String> {
  let params = account_id.map(|id| serde_json::json!({ "accountId": id }));
  rpc_call_in_background("account/usage/refresh", addr, params).await
}

#[tauri::command]
async fn service_requestlog_list(
  addr: Option<String>,
  query: Option<String>,
  limit: Option<i64>,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "query": query, "limit": limit });
  rpc_call_in_background("requestlog/list", addr, Some(params)).await
}

#[tauri::command]
async fn service_rpc_token() -> Result<String, String> {
  Ok(codexmanager_service::rpc_auth_token().to_string())
}

#[tauri::command]
async fn service_requestlog_clear(addr: Option<String>) -> Result<serde_json::Value, String> {
  rpc_call_in_background("requestlog/clear", addr, None).await
}

#[tauri::command]
async fn service_requestlog_today_summary(addr: Option<String>) -> Result<serde_json::Value, String> {
  rpc_call_in_background("requestlog/today_summary", addr, None).await
}

#[tauri::command]
async fn service_gateway_route_strategy_get(
  addr: Option<String>,
) -> Result<serde_json::Value, String> {
  rpc_call_in_background("gateway/routeStrategy/get", addr, None).await
}

#[tauri::command]
async fn service_gateway_route_strategy_set(
  addr: Option<String>,
  strategy: String,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "strategy": strategy });
  rpc_call_in_background("gateway/routeStrategy/set", addr, Some(params)).await
}

#[tauri::command]
async fn service_login_start(
  addr: Option<String>,
  login_type: String,
  open_browser: Option<bool>,
  note: Option<String>,
  tags: Option<String>,
  group_name: Option<String>,
  workspace_id: Option<String>,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({
    "type": login_type,
    "openBrowser": open_browser.unwrap_or(true),
    "note": note,
    "tags": tags,
    "groupName": group_name,
    "workspaceId": workspace_id
  });
  rpc_call_in_background("account/login/start", addr, Some(params)).await
}

#[tauri::command]
async fn service_login_status(addr: Option<String>, login_id: String) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({
    "loginId": login_id
  });
  rpc_call_in_background("account/login/status", addr, Some(params)).await
}

#[tauri::command]
async fn service_login_complete(
  addr: Option<String>,
  state: String,
  code: String,
  redirect_uri: Option<String>,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({
    "state": state,
    "code": code,
    "redirectUri": redirect_uri
  });
  rpc_call_in_background("account/login/complete", addr, Some(params)).await
}

#[tauri::command]
async fn service_apikey_list(addr: Option<String>) -> Result<serde_json::Value, String> {
  rpc_call_in_background("apikey/list", addr, None).await
}

#[tauri::command]
async fn service_apikey_read_secret(
  addr: Option<String>,
  key_id: String,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "id": key_id });
  rpc_call_in_background("apikey/readSecret", addr, Some(params)).await
}

#[tauri::command]
async fn service_apikey_create(
  addr: Option<String>,
  name: Option<String>,
  model_slug: Option<String>,
  reasoning_effort: Option<String>,
  protocol_type: Option<String>,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({
    "name": name,
    "modelSlug": model_slug,
    "reasoningEffort": reasoning_effort,
    "protocolType": protocol_type,
  });
  rpc_call_in_background("apikey/create", addr, Some(params)).await
}

#[tauri::command]
async fn service_apikey_models(
  addr: Option<String>,
  refresh_remote: Option<bool>,
) -> Result<serde_json::Value, String> {
  let params = refresh_remote.map(|value| serde_json::json!({ "refreshRemote": value }));
  rpc_call_in_background("apikey/models", addr, params).await
}

#[tauri::command]
async fn service_apikey_update_model(
  addr: Option<String>,
  key_id: String,
  model_slug: Option<String>,
  reasoning_effort: Option<String>,
  protocol_type: Option<String>,
) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({
    "id": key_id,
    "modelSlug": model_slug,
    "reasoningEffort": reasoning_effort,
    "protocolType": protocol_type,
  });
  rpc_call_in_background("apikey/updateModel", addr, Some(params)).await
}

#[tauri::command]
async fn service_apikey_delete(addr: Option<String>, key_id: String) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "id": key_id });
  rpc_call_in_background("apikey/delete", addr, Some(params)).await
}

#[tauri::command]
async fn service_apikey_disable(addr: Option<String>, key_id: String) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "id": key_id });
  rpc_call_in_background("apikey/disable", addr, Some(params)).await
}

#[tauri::command]
async fn service_apikey_enable(addr: Option<String>, key_id: String) -> Result<serde_json::Value, String> {
  let params = serde_json::json!({ "id": key_id });
  rpc_call_in_background("apikey/enable", addr, Some(params)).await
}

#[tauri::command]
async fn open_in_browser(url: String) -> Result<(), String> {
  tauri::async_runtime::spawn_blocking(move || {
    open_in_browser_blocking(&url)
  })
  .await
  .map_err(|err| format!("open_in_browser task failed: {err}"))?
}

fn open_in_browser_blocking(url: &str) -> Result<(), String> {
  if cfg!(target_os = "windows") {
    let status = std::process::Command::new("rundll32.exe")
      .args(["url.dll,FileProtocolHandler", url])
      .status()
      .map_err(|e| e.to_string())?;
    if status.success() {
      Ok(())
    } else {
      Err(format!("rundll32 failed with status: {status}"))
    }
  } else {
    webbrowser::open(url).map(|_| ()).map_err(|e| e.to_string())
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      load_env_from_exe_dir();
      app.handle().plugin(
        tauri_plugin_log::Builder::default()
          .level(log::LevelFilter::Info)
          .targets([
            tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
          ])
          .build(),
      )?;
      if let Ok(log_dir) = app.path().app_log_dir() {
        log::info!("log dir: {}", log_dir.display());
      }

      Ok(())
    })
    .on_window_event(|_window, event| {
      if let tauri::WindowEvent::CloseRequested { .. } = event {
        stop_service();
      }
      if let tauri::WindowEvent::Destroyed = event {
        stop_service();
      }
    })
    .invoke_handler(tauri::generate_handler![
      service_start,
      service_stop,
      service_initialize,
      service_account_list,
      service_account_delete,
      service_account_update,
      service_account_import,
      local_account_delete,
      service_usage_read,
      service_usage_list,
      service_usage_refresh,
      service_rpc_token,
      service_requestlog_list,
      service_requestlog_clear,
      service_requestlog_today_summary,
      service_gateway_route_strategy_get,
      service_gateway_route_strategy_set,
      service_login_start,
      service_login_status,
      service_login_complete,
      service_apikey_list,
      service_apikey_read_secret,
      service_apikey_create,
      service_apikey_models,
      service_apikey_update_model,
      service_apikey_delete,
      service_apikey_disable,
      service_apikey_enable,
      open_in_browser,
      updater::app_update_check,
      updater::app_update_prepare,
      updater::app_update_apply_portable,
      updater::app_update_launch_installer,
      updater::app_update_status
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

fn load_env_from_exe_dir() {
  let exe_path = match std::env::current_exe() {
    Ok(p) => p,
    Err(err) => {
      log::warn!("Failed to resolve current exe path: {}", err);
      return;
    }
  };
  let Some(exe_dir) = exe_path.parent() else {
    return;
  };

  // Portable-friendly env injection: if a file exists next to the exe, load KEY=VALUE pairs
  // into process environment so the embedded service (gateway) can read them.
  //
  // This avoids relying on global/system env vars when distributing a portable folder.
  // File names (first match wins): codexmanager.env, CodexManager.env, .env
  let candidates = ["codexmanager.env", "CodexManager.env", ".env"];
  let mut chosen = None;
  for name in candidates {
    let p = exe_dir.join(name);
    if p.is_file() {
      chosen = Some(p);
      break;
    }
  }
  let Some(path) = chosen else {
    return;
  };

  let bytes = match std::fs::read(&path) {
    Ok(v) => v,
    Err(err) => {
      log::warn!("Failed to read env file {}: {}", path.display(), err);
      return;
    }
  };
  let content = String::from_utf8_lossy(&bytes);
  let mut applied = 0usize;
  for (idx, raw_line) in content.lines().enumerate() {
    let line_no = idx + 1;
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
      continue;
    }
    let Some((key_raw, value_raw)) = line.split_once('=') else {
      log::warn!(
        "Skip invalid env line {}:{} (missing '=')",
        path.display(),
        line_no
      );
      continue;
    };
    let key = key_raw.trim();
    if key.is_empty() {
      continue;
    }
    let mut value = value_raw.trim().to_string();
    if (value.starts_with('\"') && value.ends_with('\"') && value.len() >= 2)
      || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
    {
      value = value[1..value.len() - 1].to_string();
    }

    // Do not override already-defined env vars (system/user-level wins).
    if std::env::var_os(key).is_some() {
      continue;
    }
    std::env::set_var(key, value);
    applied += 1;
  }

  if applied > 0 {
    log::info!("Loaded {} env vars from {}", applied, path.display());
  }
}

fn spawn_service_with_addr(app: &tauri::AppHandle, addr: &str) -> Result<(), String> {
  if std::env::var("CODEXMANAGER_NO_SERVICE").is_ok() {
    return Ok(());
  }

  if let Ok(data_path) = resolve_db_path_with_legacy_migration(app) {
    std::env::set_var("CODEXMANAGER_DB_PATH", data_path);
    if let Ok(path) = std::env::var("CODEXMANAGER_DB_PATH") {
      log::info!("db path: {}", path);
    }
  }

  std::env::set_var("CODEXMANAGER_SERVICE_ADDR", addr);
  codexmanager_service::clear_shutdown_flag();

  let addr = addr.to_string();
  let thread_addr = addr.clone();
  log::info!("service starting at {}", addr);
  let handle = thread::spawn(move || {
    if let Err(err) = codexmanager_service::start_server(&thread_addr) {
      log::error!("service stopped: {}", err);
    }
  });
  set_service_runtime(ServiceRuntime { addr, join: handle });
  Ok(())
}

fn resolve_db_path_with_legacy_migration(app: &tauri::AppHandle) -> Result<PathBuf, String> {
  let mut data_dir = app
    .path()
    .app_data_dir()
    .map_err(|_| "app data dir not found".to_string())?;
  if let Err(err) = fs::create_dir_all(&data_dir) {
    log::warn!("Failed to create app data dir: {}", err);
  }
  data_dir.push("codexmanager.db");
  maybe_migrate_legacy_db(&data_dir);
  Ok(data_dir)
}

fn maybe_migrate_legacy_db(current_db: &Path) {
  let current_has_data = db_has_user_data(current_db);
  if current_has_data {
    return;
  }

  let needs_bootstrap = !current_db.is_file() || !current_has_data;
  if !needs_bootstrap {
    return;
  }

  for legacy_db in legacy_db_candidates(current_db) {
    if !legacy_db.is_file() {
      continue;
    }
    if !db_has_user_data(&legacy_db) {
      continue;
    }

    if let Some(parent) = current_db.parent() {
      let _ = fs::create_dir_all(parent);
    }

    if current_db.is_file() {
      let backup = current_db.with_extension("db.empty.bak");
      if let Err(err) = fs::copy(current_db, &backup) {
        log::warn!(
          "Failed to backup empty current db {} -> {}: {}",
          current_db.display(),
          backup.display(),
          err
        );
      }
    }

    match fs::copy(&legacy_db, current_db) {
      Ok(_) => {
        log::info!(
          "Migrated legacy db {} -> {}",
          legacy_db.display(),
          current_db.display()
        );
        return;
      }
      Err(err) => {
        log::warn!(
          "Failed to migrate legacy db {} -> {}: {}",
          legacy_db.display(),
          current_db.display(),
          err
        );
      }
    }
  }
}

fn db_has_user_data(path: &Path) -> bool {
  if !path.is_file() {
    return false;
  }
  let storage = match Storage::open(path) {
    Ok(storage) => storage,
    Err(_) => return false,
  };
  let _ = storage.init();
  storage
    .list_accounts()
    .map(|items| !items.is_empty())
    .unwrap_or(false)
    || storage
      .list_tokens()
      .map(|items| !items.is_empty())
      .unwrap_or(false)
    || storage
      .list_api_keys()
      .map(|items| !items.is_empty())
      .unwrap_or(false)
}

fn legacy_db_candidates(current_db: &Path) -> Vec<PathBuf> {
  let mut out = Vec::new();

  if let Some(parent) = current_db.parent() {
    out.push(parent.join("gpttools.db"));
    if parent
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| name.eq_ignore_ascii_case("com.codexmanager.desktop"))
    {
      if let Some(root) = parent.parent() {
        out.push(root.join("com.gpttools.desktop").join("gpttools.db"));
      }
    }
  }

  out.retain(|candidate| candidate != current_db);
  let mut dedup = Vec::new();
  for candidate in out {
    if !dedup.iter().any(|item| item == &candidate) {
      dedup.push(candidate);
    }
  }
  dedup
}

fn normalize_addr(raw: &str) -> Result<String, String> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return Err("addr is empty".to_string());
  }
  let mut value = trimmed;
  if let Some(rest) = value.strip_prefix("http://") {
    value = rest;
  }
  if let Some(rest) = value.strip_prefix("https://") {
    value = rest;
  }
  let value = value.split('/').next().unwrap_or(value);
  if value.contains(':') {
    Ok(normalize_host(value))
  } else {
    Ok(format!("localhost:{value}"))
  }
}

fn resolve_service_addr(addr: Option<String>) -> Result<String, String> {
  if let Some(addr) = addr {
    return normalize_addr(&addr);
  }
  if let Ok(env_addr) = std::env::var("CODEXMANAGER_SERVICE_ADDR") {
    if let Ok(addr) = normalize_addr(&env_addr) {
      return Ok(addr);
    }
  }
  Ok(codexmanager_service::DEFAULT_ADDR.to_string())
}

fn split_http_response(buf: &str) -> Option<(&str, &str)> {
  if let Some((headers, body)) = buf.split_once("\r\n\r\n") {
    return Some((headers, body));
  }
  if let Some((headers, body)) = buf.split_once("\n\n") {
    return Some((headers, body));
  }
  None
}

fn response_uses_chunked(headers: &str) -> bool {
  headers.lines().any(|line| {
    let Some((name, value)) = line.split_once(':') else {
      return false;
    };
    name.trim().eq_ignore_ascii_case("transfer-encoding")
      && value.to_ascii_lowercase().contains("chunked")
  })
}

fn decode_chunked_body(raw: &str) -> Result<String, String> {
  let bytes = raw.as_bytes();
  let mut cursor = 0usize;
  let mut out = Vec::<u8>::new();

  loop {
    let Some(line_end_rel) = bytes[cursor..].windows(2).position(|w| w == b"\r\n") else {
      return Err("Invalid chunked body: missing chunk size line".to_string());
    };
    let line_end = cursor + line_end_rel;
    let line = std::str::from_utf8(&bytes[cursor..line_end])
      .map_err(|err| format!("Invalid chunked body: chunk size is not utf8 ({err})"))?;
    let size_hex = line.split(';').next().unwrap_or("").trim();
    let size = usize::from_str_radix(size_hex, 16)
      .map_err(|_| format!("Invalid chunked body: bad chunk size '{size_hex}'"))?;
    cursor = line_end + 2;
    if size == 0 {
      break;
    }
    let end = cursor.saturating_add(size);
    if end + 2 > bytes.len() {
      return Err("Invalid chunked body: truncated chunk payload".to_string());
    }
    out.extend_from_slice(&bytes[cursor..end]);
    if &bytes[end..end + 2] != b"\r\n" {
      return Err("Invalid chunked body: missing chunk terminator".to_string());
    }
    cursor = end + 2;
  }

  String::from_utf8(out).map_err(|err| format!("Invalid chunked body utf8 payload: {err}"))
}

fn parse_http_body(buf: &str) -> Result<String, String> {
  let Some((headers, body_raw)) = split_http_response(buf) else {
    // 中文注释：旧实现按原始 socket 读取，理论上总是 HTTP 报文；但在代理/半关闭边界上可能只拿到 body。
    // 这里回退为“整段按 body 处理”，避免把可解析的 JSON 误判成 malformed。
    return Ok(buf.to_string());
  };
  if response_uses_chunked(headers) {
    decode_chunked_body(body_raw)
  } else {
    Ok(body_raw.to_string())
  }
}

fn rpc_call(
  method: &str,
  addr: Option<String>,
  params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
  let addr = resolve_service_addr(addr)?;
  for attempt in 0..=1 {
    let mut stream = connect_with_timeout(&addr, Duration::from_millis(400)).map_err(|e| {
      log::warn!("rpc connect failed ({} -> {}): {}", method, addr, e);
      e
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let req = JsonRpcRequest {
      id: 1,
      method: method.to_string(),
      params: params.clone(),
    };
    let json = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    let rpc_token = codexmanager_service::rpc_auth_token();
    let http = format!(
      "POST /rpc HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nX-CodexManager-Rpc-Token: {rpc_token}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
      json.len(),
      json
    );
    stream.write_all(http.as_bytes()).map_err(|e| {
      let msg = e.to_string();
      log::warn!("rpc write failed ({} -> {}): {}", method, addr, msg);
      msg
    })?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).map_err(|e| {
      let msg = e.to_string();
      log::warn!("rpc read failed ({} -> {}): {}", method, addr, msg);
      msg
    })?;
    let body = parse_http_body(&buf).map_err(|msg| {
      log::warn!("rpc parse failed ({} -> {}): {}", method, addr, msg);
      msg
    })?;
    if body.trim().is_empty() {
      // 中文注释：前置代理在启动切换窗口可能返回空包；这里短重试一次，避免 UI 直接报“连接失败”。
      if attempt == 0 {
        std::thread::sleep(Duration::from_millis(120));
        continue;
      }
      log::warn!("rpc empty response ({} -> {})", method, addr);
      return Err("Empty response from service".to_string());
    }

    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
      let msg = e.to_string();
      log::warn!("rpc json parse failed ({} -> {}): {}", method, addr, msg);
      msg
    })?;
    if let Some(err) = v.get("error") {
      log::warn!("rpc error ({} -> {}): {}", method, addr, err);
    }
    return Ok(v);
  }

  Err("Empty response from service".to_string())
}

fn normalize_host(value: &str) -> String {
  if let Some((host, port)) = value.rsplit_once(':') {
    let mapped = match host {
      "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]" => "localhost",
      _ => host,
    };
    format!("{mapped}:{port}")
  } else {
    value.to_string()
  }
}

struct ServiceRuntime {
  addr: String,
  join: thread::JoinHandle<()>,
}

static SERVICE_RUNTIME: OnceLock<Mutex<Option<ServiceRuntime>>> = OnceLock::new();

fn set_service_runtime(runtime: ServiceRuntime) {
  let slot = SERVICE_RUNTIME.get_or_init(|| Mutex::new(None));
  if let Ok(mut guard) = slot.lock() {
    *guard = Some(runtime);
  }
}

fn take_service_runtime() -> Option<ServiceRuntime> {
  let slot = SERVICE_RUNTIME.get_or_init(|| Mutex::new(None));
  if let Ok(mut guard) = slot.lock() {
    guard.take()
  } else {
    None
  }
}

fn stop_service() {
  if let Some(runtime) = take_service_runtime() {
    log::info!("service stopping at {}", runtime.addr);
    codexmanager_service::request_shutdown(&runtime.addr);
    thread::spawn(move || {
      let _ = runtime.join.join();
    });
  }
}

fn connect_with_timeout(addr: &str, timeout: Duration) -> Result<TcpStream, String> {
  let addrs = addr
    .to_socket_addrs()
    .map_err(|err| format!("Invalid service address {addr}: {err}"))?;
  let mut last_err: Option<std::io::Error> = None;
  for sock in addrs {
    match TcpStream::connect_timeout(&sock, timeout) {
      Ok(stream) => return Ok(stream),
      Err(err) => last_err = Some(err),
    }
  }
  Err(format!(
    "Failed to connect to service at {addr}: {}",
    last_err
      .map(|e| e.to_string())
      .unwrap_or_else(|| "no address resolved".to_string())
  ))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::{Read, Write};
  use std::net::TcpListener;
  use std::time::Duration;

  #[test]
  fn normalize_addr_defaults_to_localhost() {
    assert_eq!(normalize_addr("5050").unwrap(), "localhost:5050");
    assert_eq!(
      normalize_addr("localhost:5050").unwrap(),
      "localhost:5050"
    );
  }

  #[test]
  fn rpc_call_tolerates_slow_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
      if let Ok((mut stream, _)) = listener.accept() {
        let mut buf = [0u8; 512];
        let _ = stream.read(&mut buf);
        std::thread::sleep(Duration::from_secs(3));
        let body = r#"{"result":{"ok":true}}"#;
        let response = format!(
          "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
          body.len(),
          body
        );
        let _ = stream.write_all(response.as_bytes());
      }
    });

    let res = rpc_call("initialize", Some(addr.to_string()), None);
    assert!(res.is_ok());
  }

  #[test]
  fn rpc_call_handles_chunked_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
      if let Ok((mut stream, _)) = listener.accept() {
        let mut buf = [0u8; 1024];
        let read_n = stream.read(&mut buf).expect("read");
        let request = String::from_utf8_lossy(&buf[..read_n]).to_string();
        assert!(
          request.to_ascii_lowercase().contains("connection: close"),
          "request should require connection close: {request}"
        );

        let body = r#"{"result":{"ok":true}}"#;
        let chunk_size = format!("{:X}", body.len());
        let response = format!(
          "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{chunk_size}\r\n{body}\r\n0\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes());
      }
    });

    let res = rpc_call("initialize", Some(addr.to_string()), None).expect("rpc_call");
    let ok = res
      .get("result")
      .and_then(|v| v.get("ok"))
      .and_then(|v| v.as_bool());
    assert_eq!(ok, Some(true));
  }
}
