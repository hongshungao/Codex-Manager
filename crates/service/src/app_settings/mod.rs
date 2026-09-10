mod api;
mod codex_latest_sync;
mod env_overrides;
mod gateway;
mod runtime_sync;
mod service;
mod shared;
mod store;
mod ui;

pub use api::{
    app_settings_get, app_settings_get_with_overrides, app_settings_set, author_content_get,
};
pub(crate) use codex_latest_sync::ensure_codex_latest_version_sync;
pub(crate) use env_overrides::{
    apply_env_overrides_to_process, persisted_env_overrides_missing_process_env,
    reload_runtime_after_env_override_apply,
};
pub(crate) use gateway::sync_gateway_user_agent_version_from_codex_latest;
pub(crate) use gateway::QuotaGuardInput;
#[allow(unused_imports)]
pub use gateway::{
    current_gateway_account_max_inflight, current_gateway_compact_model_forward_rules,
    current_gateway_free_account_max_model, current_gateway_model_forward_rules,
    current_gateway_originator, current_gateway_request_compression_enabled,
    current_gateway_residency_requirement, current_gateway_sse_keepalive_enabled,
    current_gateway_sse_keepalive_interval_ms,
    current_gateway_thread_aware_account_distribution_enabled,
    current_gateway_upstream_proxy_bypass_hosts, current_gateway_upstream_stream_timeout_ms,
    current_gateway_upstream_total_timeout_ms, current_gateway_user_agent,
    current_gateway_user_agent_version, default_gateway_originator, default_gateway_user_agent,
    default_gateway_user_agent_version, fetch_codex_latest_version, residency_requirement_options,
    set_gateway_account_max_inflight, set_gateway_background_tasks,
    set_gateway_compact_model_forward_rules, set_gateway_free_account_max_model,
    set_gateway_model_forward_rules, set_gateway_originator,
    set_gateway_request_compression_enabled, set_gateway_residency_requirement,
    set_gateway_route_strategy, set_gateway_sse_keepalive_enabled,
    set_gateway_sse_keepalive_interval_ms, set_gateway_thread_aware_account_distribution_enabled,
    set_gateway_upstream_proxy_bypass_hosts, set_gateway_upstream_proxy_url,
    set_gateway_upstream_stream_timeout_ms, set_gateway_upstream_total_timeout_ms,
    set_gateway_user_agent, set_gateway_user_agent_version, BackgroundTasksInput,
};
pub use runtime_sync::sync_runtime_settings_from_storage;
pub use service::{
    bind_all_interfaces_enabled, bind_all_interfaces_enabled_for_mode, current_saved_service_addr,
    current_service_bind_mode, default_listener_bind_addr, default_web_listener_addr,
    listener_bind_addr, listener_bind_addr_for_mode, set_saved_service_addr, set_service_bind_mode,
    DEFAULT_ADDR, DEFAULT_BIND_ADDR, DEFAULT_WEB_ADDR, DEFAULT_WEB_BIND_ADDR,
    SERVICE_BIND_MODE_ALL_INTERFACES, SERVICE_BIND_MODE_LOOPBACK, SERVICE_BIND_MODE_SETTING_KEY,
};
pub(crate) use shared::{normalize_optional_text, parse_bool_with_default};
pub use shared::{
    APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY, APP_SETTING_AUTHOR_SPONSORS_KEY,
    APP_SETTING_AUTO_START_ENABLED_KEY, APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
    APP_SETTING_CODEX_PROFILE_REMOVE_REQUIRES_OPENAI_AUTH_KEY,
    APP_SETTING_DISTRIBUTION_ENABLED_KEY, APP_SETTING_ENV_OVERRIDES_KEY,
    APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY, APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
    APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
    APP_SETTING_GATEWAY_ORIGINATOR_KEY, APP_SETTING_GATEWAY_QUOTA_GUARD_KEY,
    APP_SETTING_GATEWAY_REQUEST_COMPRESSION_ENABLED_KEY,
    APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
    APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY, APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY, APP_SETTING_GATEWAY_USER_AGENT_KEY,
    APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY, APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY,
    APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, APP_SETTING_PLUGIN_MARKET_MODE_KEY,
    APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY, APP_SETTING_SERVICE_ADDR_KEY,
    APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY, APP_SETTING_UI_APPEARANCE_PRESET_KEY,
    APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY, APP_SETTING_UI_LOCALE_KEY,
    APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY, APP_SETTING_UI_ZOOM_FACTOR_KEY,
    APP_SETTING_UPDATE_AUTO_CHECK_KEY, APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY,
    APP_SETTING_WEB_AUTH_MODE_KEY, WEB_ACCESS_SESSION_COOKIE_NAME,
};
pub(crate) use store::{
    get_persisted_app_setting, list_app_settings_map, save_persisted_app_setting,
    save_persisted_bool_setting,
};
pub use ui::{
    current_auto_start_enabled_setting, current_close_to_tray_on_close_setting,
    current_codex_cli_guide_dismissed, current_keep_window_ui_mounted_setting,
    current_lightweight_mode_on_close_to_tray_setting, current_show_main_window_on_startup_setting,
    current_ui_appearance_preset, current_ui_low_transparency_enabled, current_ui_theme,
    current_update_auto_check_enabled, set_auto_start_enabled_setting,
    set_close_to_tray_on_close_setting, set_codex_cli_guide_dismissed,
    set_keep_window_ui_mounted_setting, set_lightweight_mode_on_close_to_tray_setting,
    set_show_main_window_on_startup_setting, set_ui_appearance_preset,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
};
