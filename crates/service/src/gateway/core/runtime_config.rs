use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use reqwest::blocking::Client;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

static UPSTREAM_CLIENT: OnceLock<Client> = OnceLock::new();
static RUNTIME_CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static REQUEST_GATE_WAIT_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS);
static TRACE_BODY_PREVIEW_MAX_BYTES: AtomicUsize =
    AtomicUsize::new(DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES);
static FRONT_PROXY_MAX_BODY_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_FRONT_PROXY_MAX_BODY_BYTES);
static UPSTREAM_CONNECT_TIMEOUT_SECS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS);
static UPSTREAM_TOTAL_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS);
static UPSTREAM_STREAM_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS);
static ACCOUNT_MAX_INFLIGHT: AtomicUsize = AtomicUsize::new(DEFAULT_ACCOUNT_MAX_INFLIGHT);
static UPSTREAM_COOKIE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static TOKEN_EXCHANGE_CLIENT_ID: OnceLock<RwLock<String>> = OnceLock::new();
static TOKEN_EXCHANGE_ISSUER: OnceLock<RwLock<String>> = OnceLock::new();

pub(crate) const DEFAULT_MODELS_CLIENT_VERSION: &str = "0.98.0";
pub(crate) const DEFAULT_GATEWAY_DEBUG: bool = false;
const DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_ACCOUNT_MAX_INFLIGHT: usize = 0;
const DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS: u64 = 300;
const DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES: usize = 0;
const DEFAULT_FRONT_PROXY_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS";
const ENV_TRACE_BODY_PREVIEW_MAX_BYTES: &str = "CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES";
const ENV_FRONT_PROXY_MAX_BODY_BYTES: &str = "CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES";
const ENV_UPSTREAM_CONNECT_TIMEOUT_SECS: &str = "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS";
const ENV_UPSTREAM_TOTAL_TIMEOUT_MS: &str = "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS";
const ENV_UPSTREAM_STREAM_TIMEOUT_MS: &str = "CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS";
const ENV_ACCOUNT_MAX_INFLIGHT: &str = "CODEXMANAGER_ACCOUNT_MAX_INFLIGHT";
const ENV_TOKEN_EXCHANGE_CLIENT_ID: &str = "CODEXMANAGER_CLIENT_ID";
const ENV_TOKEN_EXCHANGE_ISSUER: &str = "CODEXMANAGER_ISSUER";

pub(crate) fn upstream_client() -> &'static Client {
    UPSTREAM_CLIENT.get_or_init(|| {
        ensure_runtime_config_loaded();
        build_upstream_client()
    })
}

pub(crate) fn fresh_upstream_client() -> Client {
    ensure_runtime_config_loaded();
    build_upstream_client()
}

fn upstream_connect_timeout() -> Duration {
    ensure_runtime_config_loaded();
    Duration::from_secs(UPSTREAM_CONNECT_TIMEOUT_SECS.load(Ordering::Relaxed))
}

fn build_upstream_client() -> Client {
    Client::builder()
        // 中文注释：显式关闭总超时，避免长时流式响应在客户端层被误判超时中断。
        .timeout(None::<Duration>)
        // 中文注释：连接阶段设置超时，避免网络异常时线程长期卡死占满并发槽位。
        .connect_timeout(upstream_connect_timeout())
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .build()
        .unwrap_or_else(|_| Client::new())
}

pub(crate) fn upstream_total_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = UPSTREAM_TOTAL_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

pub(crate) fn upstream_stream_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = UPSTREAM_STREAM_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

pub(crate) fn account_max_inflight_limit() -> usize {
    ensure_runtime_config_loaded();
    ACCOUNT_MAX_INFLIGHT.load(Ordering::Relaxed)
}

pub(crate) fn request_gate_wait_timeout() -> Duration {
    ensure_runtime_config_loaded();
    Duration::from_millis(REQUEST_GATE_WAIT_TIMEOUT_MS.load(Ordering::Relaxed))
}

pub(crate) fn trace_body_preview_max_bytes() -> usize {
    ensure_runtime_config_loaded();
    TRACE_BODY_PREVIEW_MAX_BYTES.load(Ordering::Relaxed)
}

pub(crate) fn front_proxy_max_body_bytes() -> usize {
    ensure_runtime_config_loaded();
    FRONT_PROXY_MAX_BODY_BYTES.load(Ordering::Relaxed)
}

pub(super) fn upstream_cookie() -> Option<String> {
    ensure_runtime_config_loaded();
    match upstream_cookie_cell().read() {
        Ok(value) => value.clone(),
        Err(_) => None,
    }
}

pub(super) fn token_exchange_client_id() -> String {
    ensure_runtime_config_loaded();
    match token_exchange_client_id_cell().read() {
        Ok(value) => value.clone(),
        Err(_) => DEFAULT_CLIENT_ID.to_string(),
    }
}

pub(super) fn token_exchange_default_issuer() -> String {
    ensure_runtime_config_loaded();
    match token_exchange_issuer_cell().read() {
        Ok(value) => value.clone(),
        Err(_) => DEFAULT_ISSUER.to_string(),
    }
}

pub(super) fn reload_from_env() {
    REQUEST_GATE_WAIT_TIMEOUT_MS.store(
        env_u64_or(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS, DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS),
        Ordering::Relaxed,
    );
    TRACE_BODY_PREVIEW_MAX_BYTES.store(
        env_usize_or(
            ENV_TRACE_BODY_PREVIEW_MAX_BYTES,
            DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES,
        ),
        Ordering::Relaxed,
    );
    FRONT_PROXY_MAX_BODY_BYTES.store(
        env_usize_or(ENV_FRONT_PROXY_MAX_BODY_BYTES, DEFAULT_FRONT_PROXY_MAX_BODY_BYTES),
        Ordering::Relaxed,
    );
    UPSTREAM_CONNECT_TIMEOUT_SECS.store(
        env_u64_or(
            ENV_UPSTREAM_CONNECT_TIMEOUT_SECS,
            DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS,
        ),
        Ordering::Relaxed,
    );
    UPSTREAM_TOTAL_TIMEOUT_MS.store(
        env_u64_or(ENV_UPSTREAM_TOTAL_TIMEOUT_MS, DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS),
        Ordering::Relaxed,
    );
    UPSTREAM_STREAM_TIMEOUT_MS.store(
        env_u64_or(ENV_UPSTREAM_STREAM_TIMEOUT_MS, DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS),
        Ordering::Relaxed,
    );
    ACCOUNT_MAX_INFLIGHT.store(
        env_usize_or(ENV_ACCOUNT_MAX_INFLIGHT, DEFAULT_ACCOUNT_MAX_INFLIGHT),
        Ordering::Relaxed,
    );

    let cookie = env_non_empty(ENV_UPSTREAM_COOKIE);
    if let Ok(mut cached) = upstream_cookie_cell().write() {
        *cached = cookie;
    }

    let client_id = env_non_empty(ENV_TOKEN_EXCHANGE_CLIENT_ID)
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());
    if let Ok(mut cached) = token_exchange_client_id_cell().write() {
        *cached = client_id;
    }

    let issuer = env_non_empty(ENV_TOKEN_EXCHANGE_ISSUER)
        .unwrap_or_else(|| DEFAULT_ISSUER.to_string());
    if let Ok(mut cached) = token_exchange_issuer_cell().write() {
        *cached = issuer;
    }
}

const ENV_UPSTREAM_COOKIE: &str = "CODEXMANAGER_UPSTREAM_COOKIE";

fn ensure_runtime_config_loaded() {
    let _ = RUNTIME_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

fn upstream_cookie_cell() -> &'static RwLock<Option<String>> {
    UPSTREAM_COOKIE.get_or_init(|| RwLock::new(None))
}

fn token_exchange_client_id_cell() -> &'static RwLock<String> {
    TOKEN_EXCHANGE_CLIENT_ID.get_or_init(|| RwLock::new(DEFAULT_CLIENT_ID.to_string()))
}

fn token_exchange_issuer_cell() -> &'static RwLock<String> {
    TOKEN_EXCHANGE_ISSUER.get_or_init(|| RwLock::new(DEFAULT_ISSUER.to_string()))
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_u64_or(name: &str, default: u64) -> u64 {
    env_non_empty(name)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize_or(name: &str, default: usize) -> usize {
    env_non_empty(name)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn reload_from_env_updates_timeout_and_cookie() {
        let _timeout_guard = EnvGuard::set(ENV_UPSTREAM_TOTAL_TIMEOUT_MS, "777");
        let _stream_timeout_guard = EnvGuard::set(ENV_UPSTREAM_STREAM_TIMEOUT_MS, "888");
        let _cookie_guard = EnvGuard::set(ENV_UPSTREAM_COOKIE, "cookie=abc");
        let _client_id_guard = EnvGuard::set(ENV_TOKEN_EXCHANGE_CLIENT_ID, "client-id-123");
        let _issuer_guard = EnvGuard::set(ENV_TOKEN_EXCHANGE_ISSUER, "https://issuer.example");

        reload_from_env();

        assert_eq!(upstream_total_timeout(), Some(Duration::from_millis(777)));
        assert_eq!(upstream_stream_timeout(), Some(Duration::from_millis(888)));
        assert_eq!(upstream_cookie().as_deref(), Some("cookie=abc"));
        assert_eq!(token_exchange_client_id(), "client-id-123");
        assert_eq!(
            token_exchange_default_issuer(),
            "https://issuer.example".to_string()
        );
    }
}
