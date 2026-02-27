use codexmanager_core::storage::Storage;
use std::path::Path;
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use rand::RngCore;
use sha2::{Digest, Sha256};

struct CachedStorage {
    path: String,
    storage: Storage,
}

thread_local! {
    static STORAGE_CACHE: RefCell<Option<CachedStorage>> = const { RefCell::new(None) };
}

pub(crate) struct StorageHandle {
    path: String,
    storage: Option<Storage>,
}

impl StorageHandle {
    fn new(path: String, storage: Storage) -> Self {
        Self {
            path,
            storage: Some(storage),
        }
    }
}

impl Deref for StorageHandle {
    type Target = Storage;

    fn deref(&self) -> &Self::Target {
        self.storage.as_ref().expect("storage handle should exist")
    }
}

impl DerefMut for StorageHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.storage.as_mut().expect("storage handle should exist")
    }
}

impl Drop for StorageHandle {
    fn drop(&mut self) {
        let Some(storage) = self.storage.take() else {
            return;
        };
        let path = self.path.clone();
        STORAGE_CACHE.with(|cell| {
            let mut cache = cell.borrow_mut();
            *cache = Some(CachedStorage { path, storage });
        });
    }
}

fn normalize_key_part(value: Option<&str>) -> Option<String> {
    // 规范化 key 片段，去除空白并避免分隔符冲突
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.replace("::", "_"))
}

fn compact_key_part(value: &str) -> String {
    // 对过长/复杂后缀做短哈希，避免账号ID过长且保留稳定唯一性。
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let should_hash = trimmed.len() > 16
        || trimmed.contains('|')
        || trimmed.contains('-')
        || trimmed.contains(' ');
    if !should_hash {
        return trimmed.to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(12);
    for b in digest.iter().take(6) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub(crate) fn account_key(account_id: &str, tags: Option<&str>) -> String {
    // 组合账号与标签，生成稳定的账户唯一标识
    let mut parts = Vec::new();
    parts.push(account_id.to_string());
    if let Some(value) = normalize_key_part(tags) {
        let compact = compact_key_part(&value);
        if !compact.is_empty() {
            parts.push(compact);
        }
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

#[cfg(test)]
static STORAGE_OPEN_COUNTS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, usize>>> =
    std::sync::OnceLock::new();

pub(crate) fn open_storage() -> Option<StorageHandle> {
    // 读取数据库路径并打开存储
    let path = match std::env::var("CODEXMANAGER_DB_PATH") {
        Ok(path) => path,
        Err(_) => {
            log::warn!("CODEXMANAGER_DB_PATH not set");
            return None;
        }
    };
    open_storage_at_path(&path)
}

fn open_storage_at_path(path: &str) -> Option<StorageHandle> {
    if let Some(storage) = take_cached_storage(&path) {
        return Some(StorageHandle::new(path.to_string(), storage));
    }

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
    #[cfg(test)]
    record_storage_open_for_tests(path);
    Some(StorageHandle::new(path.to_string(), storage))
}

pub(crate) fn initialize_storage() -> Result<(), String> {
    let path = std::env::var("CODEXMANAGER_DB_PATH")
        .map_err(|_| "CODEXMANAGER_DB_PATH not set".to_string())?;
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

fn take_cached_storage(path: &str) -> Option<Storage> {
    STORAGE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        match cache.take() {
            Some(CachedStorage {
                path: cached_path,
                storage,
            }) if cached_path == path => Some(storage),
            Some(other) => {
                *cache = Some(other);
                None
            }
            None => None,
        }
    })
}

#[cfg(test)]
fn clear_storage_cache_for_tests() {
    STORAGE_CACHE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[cfg(test)]
fn record_storage_open_for_tests(path: &str) {
    let mutex = STORAGE_OPEN_COUNTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    let entry = counts.entry(path.to_string()).or_insert(0);
    *entry += 1;
}

#[cfg(test)]
fn storage_open_count_for_tests(path: &str) -> usize {
    let Some(mutex) = STORAGE_OPEN_COUNTS.get() else {
        return 0;
    };
    let counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    counts.get(path).copied().unwrap_or(0)
}

#[cfg(test)]
fn clear_storage_open_count_for_tests(path: &str) {
    let Some(mutex) = STORAGE_OPEN_COUNTS.get() else {
        return;
    };
    let mut counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    counts.remove(path);
}

#[cfg(test)]
mod tests {
    use super::{
        clear_storage_cache_for_tests, clear_storage_open_count_for_tests, open_storage_at_path,
        storage_open_count_for_tests,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_db_path(prefix: &str) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("{prefix}-{nonce}.db"))
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn open_storage_reuses_cached_connection_in_same_thread() {
        let db_path = unique_db_path("codexmanager-open-storage-reuse");
        clear_storage_cache_for_tests();
        clear_storage_open_count_for_tests(&db_path);

        let storage = open_storage_at_path(&db_path).expect("open storage 1");
        storage.init().expect("init");
        drop(storage);

        let storage = open_storage_at_path(&db_path).expect("open storage 2");
        drop(storage);

        assert_eq!(storage_open_count_for_tests(&db_path), 1);

        clear_storage_cache_for_tests();
        clear_storage_open_count_for_tests(&db_path);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn open_storage_reopens_when_db_path_changes() {
        let db_path_1 = unique_db_path("codexmanager-open-storage-path-1");
        let db_path_2 = unique_db_path("codexmanager-open-storage-path-2");
        clear_storage_cache_for_tests();
        clear_storage_open_count_for_tests(&db_path_1);
        clear_storage_open_count_for_tests(&db_path_2);

        let storage = open_storage_at_path(&db_path_1).expect("open storage path 1");
        storage.init().expect("init 1");
        drop(storage);

        let storage = open_storage_at_path(&db_path_2).expect("open storage path 2");
        storage.init().expect("init 2");
        drop(storage);

        assert_eq!(storage_open_count_for_tests(&db_path_1), 1);
        assert_eq!(storage_open_count_for_tests(&db_path_2), 1);

        clear_storage_cache_for_tests();
        clear_storage_open_count_for_tests(&db_path_1);
        clear_storage_open_count_for_tests(&db_path_2);
        let _ = std::fs::remove_file(&db_path_1);
        let _ = std::fs::remove_file(&db_path_2);
    }
}
