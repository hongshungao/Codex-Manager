use gpttools_core::storage::Storage;
use std::path::Path;
use rand::RngCore;
use sha2::{Digest, Sha256};

fn normalize_key_part(value: Option<&str>) -> Option<String> {
    // 规范化 key 片段，去除空白并避免分隔符冲突
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.replace("::", "_"))
}

pub(crate) fn account_key(account_id: &str, tags: Option<&str>) -> String {
    // 组合账号与标签，生成稳定的账户唯一标识
    let mut parts = Vec::new();
    parts.push(account_id.to_string());
    if let Some(value) = normalize_key_part(tags) {
        parts.push(value);
    }
    parts.join("::")
}

pub(crate) fn hash_platform_key(key: &str) -> String {
    // 对平台 Key 做不可逆哈希，避免明文存储
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub(crate) fn generate_platform_key() -> String {
    // 生成随机平台 Key（十六进制）
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::with_capacity(buf.len() * 2);
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub(crate) fn generate_key_id() -> String {
    // 生成短 ID 作为平台 Key 的展示标识
    let mut buf = [0u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::from("gk_");
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub(crate) fn open_storage() -> Option<Storage> {
    // 读取数据库路径并打开存储
    let path = match std::env::var("GPTTOOLS_DB_PATH") {
        Ok(path) => path,
        Err(_) => {
            log::warn!("GPTTOOLS_DB_PATH not set");
            return None;
        }
    };
    if !Path::new(&path).exists() {
        log::warn!("storage path missing: {}", path);
    }
    let storage = match Storage::open(&path) {
        Ok(storage) => storage,
        Err(err) => {
            log::error!("open storage failed: {} ({})", path, err);
            return None;
        }
    };
    Some(storage)
}

pub(crate) fn initialize_storage() -> Result<(), String> {
    let path = std::env::var("GPTTOOLS_DB_PATH")
        .map_err(|_| "GPTTOOLS_DB_PATH not set".to_string())?;
    if !Path::new(&path).exists() {
        log::warn!("storage path missing: {}", path);
    }
    let storage = Storage::open(&path)
        .map_err(|err| format!("open storage failed: {} ({})", path, err))?;
    storage
        .init()
        .map_err(|err| format!("storage init failed: {} ({})", path, err))?;
    Ok(())
}
