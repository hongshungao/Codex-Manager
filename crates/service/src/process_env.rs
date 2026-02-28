use rand::RngCore;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const ENV_CANDIDATES: [&str; 3] = ["codexmanager.env", "CodexManager.env", ".env"];
const DEFAULT_DB_FILENAME: &str = "codexmanager.db";
const DEFAULT_RPC_TOKEN_FILENAME: &str = "codexmanager.rpc-token";

pub(crate) const ENV_DB_PATH: &str = "CODEXMANAGER_DB_PATH";
pub(crate) const ENV_RPC_TOKEN: &str = "CODEXMANAGER_RPC_TOKEN";
pub(crate) const ENV_RPC_TOKEN_FILE: &str = "CODEXMANAGER_RPC_TOKEN_FILE";

pub(crate) fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn strip_inline_comment(value: &str) -> &str {
    // Only treat ` #` as comment start (common dotenv behavior).
    let Some(pos) = value.find(" #") else {
        return value;
    };
    value[..pos].trim_end()
}

fn parse_dotenv_kv(line: &str) -> Option<(String, String)> {
    let mut line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        return None;
    }
    if let Some(rest) = line.strip_prefix("export ") {
        line = rest.trim();
    }
    let (key, raw_value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut value = raw_value.trim();
    // Handle quoted values: KEY="a b", KEY='a b'
    if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
        || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
    {
        value = &value[1..value.len() - 1];
    } else {
        value = strip_inline_comment(value);
    }
    Some((key.to_string(), value.to_string()))
}

fn find_env_file_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in ENV_CANDIDATES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn load_env_from_exe_dir() {
    let dir = exe_dir();
    let Some(path) = find_env_file_in_dir(&dir) else {
        return;
    };

    let Ok(mut f) = fs::File::open(&path) else {
        return;
    };
    let mut text = String::new();
    if f.read_to_string(&mut text).is_err() {
        return;
    }

    let mut applied = 0usize;
    for line in text.lines() {
        let Some((key, value)) = parse_dotenv_kv(line) else {
            continue;
        };
        if std::env::var_os(&key).is_some() {
            continue;
        }
        std::env::set_var(key, value);
        applied += 1;
    }

    if applied > 0 {
        log::info!("Loaded {} env vars from {}", applied, path.display());
    }
}

fn resolve_path_with_base(raw: &str, base_dir: &Path) -> PathBuf {
    let raw = raw.trim();
    if raw.is_empty() {
        return PathBuf::new();
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    base_dir.join(path)
}

pub(crate) fn ensure_default_db_path() -> PathBuf {
    let dir = exe_dir();
    let resolved = match std::env::var(ENV_DB_PATH) {
        Ok(raw) if !raw.trim().is_empty() => resolve_path_with_base(&raw, &dir),
        _ => dir.join(DEFAULT_DB_FILENAME),
    };
    std::env::set_var(ENV_DB_PATH, resolved.to_string_lossy().as_ref());
    resolved
}

pub(crate) fn db_dir() -> PathBuf {
    let db_path = ensure_default_db_path();
    db_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(exe_dir)
}

pub(crate) fn rpc_token_file_path() -> PathBuf {
    if let Ok(raw) = std::env::var(ENV_RPC_TOKEN_FILE) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return resolve_path_with_base(trimmed, &db_dir());
        }
    }
    db_dir().join(DEFAULT_RPC_TOKEN_FILENAME)
}

pub(crate) fn read_rpc_token_from_file(path: &Path) -> Option<String> {
    let Ok(mut f) = fs::File::open(path) else {
        return None;
    };
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return None;
    }
    let token = buf.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

pub(crate) fn read_rpc_token_from_env_or_file() -> Option<String> {
    if let Ok(raw) = std::env::var(ENV_RPC_TOKEN) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    read_rpc_token_from_file(&rpc_token_file_path())
}

fn write_text_file(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(path)?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

pub(crate) fn persist_rpc_token_best_effort(token: &str) {
    let path = rpc_token_file_path();
    if let Err(err) = write_text_file(&path, token) {
        log::warn!(
            "persist rpc token failed: {} ({})",
            path.to_string_lossy(),
            err
        );
    }
}

pub(crate) fn generate_rpc_token_hex_32bytes() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    token
}
