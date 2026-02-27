use rand::RngCore;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// Env overrides:
// - CODEXMANAGER_PROMPT_CACHE_TTL_SECS (default: 3600)
// - CODEXMANAGER_PROMPT_CACHE_CLEANUP_INTERVAL_SECS (default: 60)
// - CODEXMANAGER_PROMPT_CACHE_CAPACITY (default: 4096; 0 disables capacity limit)
const PROMPT_CACHE_TTL_SECS_ENV: &str = "CODEXMANAGER_PROMPT_CACHE_TTL_SECS";
const PROMPT_CACHE_CLEANUP_INTERVAL_SECS_ENV: &str = "CODEXMANAGER_PROMPT_CACHE_CLEANUP_INTERVAL_SECS";
const PROMPT_CACHE_CAPACITY_ENV: &str = "CODEXMANAGER_PROMPT_CACHE_CAPACITY";

const DEFAULT_PROMPT_CACHE_TTL_SECS: u64 = 60 * 60;
const DEFAULT_PROMPT_CACHE_CLEANUP_INTERVAL_SECS: u64 = 60;
const DEFAULT_PROMPT_CACHE_CAPACITY: usize = 4096;

static PROMPT_CACHE: OnceLock<Mutex<PromptCache>> = OnceLock::new();

#[derive(Clone)]
struct PromptCacheEntry {
    id: String,
    last_seen: Instant,
    lru_tick: u64,
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
    let now = Instant::now();
    let cache = PROMPT_CACHE.get_or_init(|| Mutex::new(PromptCache::new(now)));
    let mut guard = crate::lock_utils::lock_recover(cache, "prompt_cache");
    guard.get_or_create(key, now)
}

#[derive(Clone, Copy)]
struct PromptCacheConfig {
    ttl: Duration,
    cleanup_interval: Duration,
    capacity: usize,
}

impl PromptCacheConfig {
    fn load_from_env() -> Self {
        let ttl_secs = env_u64_or(PROMPT_CACHE_TTL_SECS_ENV, DEFAULT_PROMPT_CACHE_TTL_SECS);
        let cleanup_secs = env_u64_or(
            PROMPT_CACHE_CLEANUP_INTERVAL_SECS_ENV,
            DEFAULT_PROMPT_CACHE_CLEANUP_INTERVAL_SECS,
        );
        let capacity = env_usize_or(PROMPT_CACHE_CAPACITY_ENV, DEFAULT_PROMPT_CACHE_CAPACITY);
        Self {
            ttl: Duration::from_secs(ttl_secs),
            cleanup_interval: Duration::from_secs(cleanup_secs),
            capacity,
        }
    }
}

struct PromptCache {
    by_key: HashMap<String, PromptCacheEntry>,
    // LRU ordering by monotonic tick: smallest tick = least recently seen.
    lru_by_tick: BTreeMap<u64, String>,
    tick: u64,
    last_cleanup: Instant,
    config: PromptCacheConfig,
}

impl PromptCache {
    fn new(now: Instant) -> Self {
        Self {
            by_key: HashMap::new(),
            lru_by_tick: BTreeMap::new(),
            tick: 0,
            last_cleanup: now,
            config: PromptCacheConfig::load_from_env(),
        }
    }

    fn get_or_create(&mut self, key: &str, now: Instant) -> String {
        self.maybe_cleanup(now);

        // Fast path: key hit.
        let mut expired_tick: Option<u64> = None;
        let mut touch: Option<(String, u64, u64)> = None;
        if let Some(entry) = self.by_key.get_mut(key) {
            if is_entry_expired(entry.last_seen, now, self.config.ttl) {
                // If the accessed entry is expired, drop it immediately (no full scan).
                expired_tick = Some(entry.lru_tick);
            } else {
                let old_tick = entry.lru_tick;
                self.tick = self.tick.wrapping_add(1);
                let new_tick = self.tick;
                entry.last_seen = now;
                entry.lru_tick = new_tick;
                touch = Some((entry.id.clone(), old_tick, new_tick));
            }
        }

        if let Some(stale_tick) = expired_tick {
            self.by_key.remove(key);
            self.lru_by_tick.remove(&stale_tick);
        }

        if let Some((id, old_tick, new_tick)) = touch {
            self.lru_by_tick.remove(&old_tick);
            self.lru_by_tick.insert(new_tick, key.to_string());
            return id;
        }

        // Miss: create new entry.
        let id = random_uuid_v4();
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        self.by_key.insert(
            key.to_string(),
            PromptCacheEntry {
                id: id.clone(),
                last_seen: now,
                lru_tick: tick,
            },
        );
        self.lru_by_tick.insert(tick, key.to_string());
        self.enforce_capacity();
        id
    }

    fn maybe_cleanup(&mut self, now: Instant) {
        let interval = self.config.cleanup_interval;
        if interval.is_zero()
            || now
                .checked_duration_since(self.last_cleanup)
                .is_some_and(|elapsed| elapsed >= interval)
        {
            self.cleanup(now);
        }
    }

    fn cleanup(&mut self, now: Instant) {
        self.last_cleanup = now;

        let ttl = self.config.ttl;
        if !ttl.is_zero() {
            self.by_key
                .retain(|_, entry| !is_entry_expired(entry.last_seen, now, ttl));
        }

        // Rebuild the LRU index to avoid drift (e.g. if entries were pruned).
        self.lru_by_tick.clear();
        for (key, entry) in self.by_key.iter() {
            self.lru_by_tick.insert(entry.lru_tick, key.clone());
        }

        self.enforce_capacity();
    }

    fn enforce_capacity(&mut self) {
        let cap = self.config.capacity;
        if cap == 0 {
            return;
        }
        while self.by_key.len() > cap {
            let Some((&oldest_tick, oldest_key)) = self.lru_by_tick.iter().next() else {
                break;
            };
            let oldest_key = oldest_key.clone();
            self.lru_by_tick.remove(&oldest_tick);
            self.by_key.remove(oldest_key.as_str());
        }
    }
}

fn is_entry_expired(last_seen: Instant, now: Instant, ttl: Duration) -> bool {
    if ttl.is_zero() {
        return false;
    }
    now.checked_duration_since(last_seen)
        .is_some_and(|age| age > ttl)
}

fn env_u64_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(default)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn new_test_cache(now: Instant, config: PromptCacheConfig) -> PromptCache {
        PromptCache {
            by_key: HashMap::new(),
            lru_by_tick: BTreeMap::new(),
            tick: 0,
            last_cleanup: now,
            config,
        }
    }

    #[test]
    fn lru_capacity_evicts_least_recently_seen() {
        let now = Instant::now();
        let mut cache = new_test_cache(
            now,
            PromptCacheConfig {
                ttl: Duration::ZERO,
                cleanup_interval: Duration::from_secs(3600),
                capacity: 2,
            },
        );

        let id1 = cache.get_or_create("k1", now);
        let id2 = cache.get_or_create("k2", now);
        assert_eq!(cache.by_key.len(), 2);

        // Touch k1 so k2 becomes the LRU.
        assert_eq!(cache.get_or_create("k1", now + Duration::from_secs(1)), id1);

        let id3 = cache.get_or_create("k3", now + Duration::from_secs(2));
        assert_eq!(cache.by_key.len(), 2);
        assert!(cache.by_key.contains_key("k1"));
        assert!(cache.by_key.contains_key("k3"));
        assert!(!cache.by_key.contains_key("k2"));

        // k2 should have been evicted.
        let id2_new = cache.get_or_create("k2", now + Duration::from_secs(3));
        assert_ne!(id2_new, id2);
        assert_eq!(cache.get_or_create("k3", now + Duration::from_secs(3)), id3);
        assert_ne!(cache.get_or_create("k1", now + Duration::from_secs(3)), id1);
    }

    #[test]
    fn ttl_expires_after_idle_and_is_checked_on_access() {
        let now = Instant::now();
        let mut cache = new_test_cache(
            now,
            PromptCacheConfig {
                ttl: Duration::from_secs(10),
                cleanup_interval: Duration::from_secs(3600),
                capacity: 0,
            },
        );

        let id1 = cache.get_or_create("k1", now);

        // Within TTL: hit returns same id and refreshes last_seen.
        assert_eq!(cache.get_or_create("k1", now + Duration::from_secs(9)), id1);

        // Past TTL since last_seen: miss returns new id.
        let id2 = cache.get_or_create("k1", now + Duration::from_secs(21));
        assert_ne!(id2, id1);
    }
}
