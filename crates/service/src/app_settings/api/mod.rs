mod author_links;
mod current;
mod patch;

use crate::initialize_storage_if_needed;
use serde_json::Value;

pub(super) use super::env_overrides::{
    current_env_overrides, env_override_catalog_value, env_override_reserved_keys,
    env_override_unsupported_keys, save_env_overrides_value, set_env_overrides,
};
pub(super) use super::gateway::{
    current_background_tasks_snapshot_value, current_gateway_account_max_inflight,
    current_gateway_compact_model_forward_rules, current_gateway_free_account_max_model,
    current_gateway_model_forward_rules, current_gateway_originator, current_gateway_quota_guard,
    current_gateway_residency_requirement, current_gateway_sse_keepalive_enabled,
    current_gateway_sse_keepalive_interval_ms,
    current_gateway_thread_aware_account_distribution_enabled,
    current_gateway_upstream_proxy_bypass_hosts, current_gateway_upstream_stream_timeout_ms,
    current_gateway_upstream_total_timeout_ms, current_gateway_user_agent,
    current_gateway_user_agent_version, default_gateway_originator, default_gateway_user_agent,
    default_gateway_user_agent_version, residency_requirement_options,
    set_gateway_account_max_inflight, set_gateway_background_tasks,
    set_gateway_compact_model_forward_rules, set_gateway_free_account_max_model,
    set_gateway_model_forward_rules, set_gateway_originator, set_gateway_quota_guard,
    set_gateway_residency_requirement, set_gateway_route_strategy,
    set_gateway_sse_keepalive_enabled, set_gateway_sse_keepalive_interval_ms,
    set_gateway_thread_aware_account_distribution_enabled, set_gateway_upstream_proxy_bypass_hosts,
    set_gateway_upstream_proxy_url, set_gateway_upstream_stream_timeout_ms,
    set_gateway_upstream_total_timeout_ms, set_gateway_user_agent, set_gateway_user_agent_version,
    BackgroundTasksInput, QuotaGuardInput,
};
pub(super) use super::runtime_sync::sync_runtime_settings_from_storage;
pub(super) use super::service::{
    current_saved_service_addr, current_service_bind_mode, set_saved_service_addr,
    set_service_bind_mode, SERVICE_BIND_MODE_ALL_INTERFACES, SERVICE_BIND_MODE_LOOPBACK,
    SERVICE_BIND_MODE_SETTING_KEY,
};
pub(super) use super::store::{save_persisted_app_setting, save_persisted_bool_setting};
pub(super) use super::ui::{
    normalize_ui_appearance_preset, normalize_ui_locale, normalize_ui_theme,
    normalize_ui_zoom_factor, set_auto_start_enabled_setting, set_close_to_tray_on_close_setting,
    set_codex_cli_guide_dismissed, set_keep_window_ui_mounted_setting,
    set_lightweight_mode_on_close_to_tray_setting, set_show_main_window_on_startup_setting,
    set_ui_appearance_preset, set_ui_locale, set_ui_low_transparency_enabled, set_ui_theme,
    set_ui_zoom_factor, set_update_auto_check_enabled,
};
pub(super) use super::{normalize_optional_text, parse_bool_with_default};
pub(super) use super::{
    APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY, APP_SETTING_AUTHOR_SPONSORS_KEY,
    APP_SETTING_AUTO_START_ENABLED_KEY, APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
    APP_SETTING_CODEX_PROFILE_REMOVE_REQUIRES_OPENAI_AUTH_KEY, APP_SETTING_ENV_OVERRIDES_KEY,
    APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY, APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
    APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
    APP_SETTING_GATEWAY_ORIGINATOR_KEY, APP_SETTING_GATEWAY_QUOTA_GUARD_KEY,
    APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
    APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY, APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY, APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY,
    APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY, APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY,
    APP_SETTING_PLUGIN_MARKET_MODE_KEY, APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY,
    APP_SETTING_SERVICE_ADDR_KEY, APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY,
    APP_SETTING_UI_APPEARANCE_PRESET_KEY, APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY,
    APP_SETTING_UI_LOCALE_KEY, APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY,
    APP_SETTING_UI_ZOOM_FACTOR_KEY, APP_SETTING_UPDATE_AUTO_CHECK_KEY,
};

/// 函数 `app_settings_get`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub fn app_settings_get() -> Result<Value, String> {
    current::current_app_settings_value(None, None, None)
}

/// 函数 `app_settings_get_with_overrides`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - close_to_tray_on_close: 参数 close_to_tray_on_close
/// - close_to_tray_supported: 参数 close_to_tray_supported
///
/// # 返回
/// 返回函数执行结果
pub fn app_settings_get_with_overrides(
    close_to_tray_on_close: Option<bool>,
    close_to_tray_supported: Option<bool>,
) -> Result<Value, String> {
    current::current_app_settings_value(close_to_tray_on_close, close_to_tray_supported, None)
}

pub fn author_content_get() -> Result<Value, String> {
    current::current_author_content_value()
}

/// 函数 `app_settings_set`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - params: 参数 params
///
/// # 返回
/// 返回函数执行结果
pub fn app_settings_set(params: Option<&Value>) -> Result<Value, String> {
    initialize_storage_if_needed()?;
    let patch = patch::parse_app_settings_patch(params)?;
    let service_listen_mode = patch.service_listen_mode.clone();
    patch::apply_app_settings_patch(patch)?;
    current::current_app_settings_value_persisted(None, None, service_listen_mode.as_deref())
}
