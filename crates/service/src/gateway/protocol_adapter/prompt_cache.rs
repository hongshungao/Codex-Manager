use rand::RngCore;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const PROMPT_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
static PROMPT_CACHE: OnceLock<Mutex<HashMap<String, PromptCacheEntry>>> = OnceLock::new();

#[derive(Clone)]
struct PromptCacheEntry {
    id: String,
    expires_at: Instant,
}

pub(super) fn resolve_prompt_cache_key(
    source: &serde_json::Map<String, Value>,
    model: Option<&Value>,
) -> Option<String> {
    let model = model
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let user_id = source
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("user_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown");

    let cache_key = format!("{model}:{user_id}");
    Some(get_or_create_prompt_cache_id(&cache_key))
}

fn get_or_create_prompt_cache_id(key: &str) -> String {
    let cache = PROMPT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    let mut guard = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.retain(|_, entry| entry.expires_at > now);
    if let Some(entry) = guard.get(key) {
        return entry.id.clone();
    }

    let id = random_uuid_v4();
    guard.insert(
        key.to_string(),
        PromptCacheEntry {
            id: id.clone(),
            expires_at: now + PROMPT_CACHE_TTL,
        },
    );
    id
}

fn random_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}
