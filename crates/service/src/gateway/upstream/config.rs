use reqwest::header::HeaderValue;
use std::sync::{OnceLock, RwLock};

const ENV_UPSTREAM_BASE_URL: &str = "CODEXMANAGER_UPSTREAM_BASE_URL";
const ENV_UPSTREAM_FALLBACK_BASE_URL: &str = "CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL";
const DEFAULT_UPSTREAM_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

static CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static UPSTREAM_BASE_URL: OnceLock<RwLock<String>> = OnceLock::new();
static UPSTREAM_FALLBACK_BASE_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();

pub(in super::super) fn normalize_upstream_base_url(base: &str) -> String {
    let mut normalized = base.trim().trim_end_matches('/').to_string();
    let lower = normalized.to_ascii_lowercase();
    if (lower.starts_with("https://chatgpt.com")
        || lower.starts_with("https://chat.openai.com"))
        && !lower.contains("/backend-api")
    {
        // 中文注释：对齐官方客户端的主机归一化，避免仅填域名时落到错误路径。
        normalized = format!("{normalized}/backend-api/codex");
    }
    normalized
}

pub(in super::super) fn resolve_upstream_base_url() -> String {
    ensure_config_loaded();
    crate::lock_utils::read_recover(upstream_base_url_cell(), "upstream_base_url").clone()
}

pub(in super::super) fn resolve_upstream_fallback_base_url(primary_base: &str) -> Option<String> {
    ensure_config_loaded();
    crate::lock_utils::read_recover(upstream_fallback_base_url_cell(), "upstream_fallback_base_url")
        .clone()
    .or_else(|| {
        if is_chatgpt_backend_base(primary_base) {
            // 默认兜底到 OpenAI v1，避免 Cloudflare challenge 时模型列表不可用。
            Some("https://api.openai.com/v1".to_string())
        } else {
            None
        }
    })
}

pub(in super::super) fn is_openai_api_base(base: &str) -> bool {
    let normalized = base.trim().to_ascii_lowercase();
    normalized.contains("api.openai.com/v1")
}

pub(in super::super) fn is_chatgpt_backend_base(base: &str) -> bool {
    let normalized = base.trim().to_ascii_lowercase();
    normalized.contains("chatgpt.com/backend-api")
        || normalized.contains("chat.openai.com/backend-api")
}

pub(in super::super) fn should_try_openai_fallback(
    base: &str,
    request_path: &str,
    content_type: Option<&HeaderValue>,
) -> bool {
    if !is_chatgpt_backend_base(base) {
        return false;
    }
    let is_models_path = request_path == "/v1/models" || request_path.starts_with("/v1/models?");
    if is_models_path {
        // /models 需要与官方行为一致地直接透传，避免 fallback token-exchange 影响模型列表稳定性。
        return false;
    }
    let Some(content_type) = content_type else {
        return false;
    };
    let Ok(value) = content_type.to_str() else {
        return false;
    };
    super::super::is_html_content_type(value)
}

pub(in super::super) fn should_try_openai_fallback_by_status(
    base: &str,
    request_path: &str,
    status_code: u16,
) -> bool {
    if !is_chatgpt_backend_base(base) {
        return false;
    }
    let is_models_path = request_path == "/v1/models" || request_path.starts_with("/v1/models?");
    if is_models_path {
        return false;
    }
    if status_code == 429 {
        return true;
    }
    if status_code == 401 || status_code == 403 {
        // 中文注释：/v1/responses 在部分账号上会先返回 401/403（content-type 未必是 text/html），
        // 若只依赖 content-type 触发 fallback，会直接落到 challenge blocked。
        return request_path.starts_with("/v1/responses");
    }
    false
}

pub(in super::super) fn reload_from_env() {
    let base = env_non_empty(ENV_UPSTREAM_BASE_URL)
        .map(|value| normalize_upstream_base_url(&value))
        .unwrap_or_else(|| DEFAULT_UPSTREAM_BASE_URL.to_string());
    let mut cached_base =
        crate::lock_utils::write_recover(upstream_base_url_cell(), "upstream_base_url");
    *cached_base = base;

    let fallback = env_non_empty(ENV_UPSTREAM_FALLBACK_BASE_URL)
        .map(|value| normalize_upstream_base_url(&value));
    let mut cached_fallback = crate::lock_utils::write_recover(
        upstream_fallback_base_url_cell(),
        "upstream_fallback_base_url",
    );
    *cached_fallback = fallback;
}

fn ensure_config_loaded() {
    let _ = CONFIG_LOADED.get_or_init(|| reload_from_env());
}

fn upstream_base_url_cell() -> &'static RwLock<String> {
    UPSTREAM_BASE_URL.get_or_init(|| RwLock::new(DEFAULT_UPSTREAM_BASE_URL.to_string()))
}

fn upstream_fallback_base_url_cell() -> &'static RwLock<Option<String>> {
    UPSTREAM_FALLBACK_BASE_URL.get_or_init(|| RwLock::new(None))
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}



