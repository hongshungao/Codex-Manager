pub const APP_SETTING_UPDATE_AUTO_CHECK_KEY: &str = "app.update.auto_check";
pub const APP_SETTING_AUTO_START_ENABLED_KEY: &str = "app.auto_start_enabled";
pub const APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY: &str = "app.show_main_window_on_startup";
pub const APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY: &str = "app.close_to_tray_on_close";
pub const APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY: &str = "app.keep_window_ui_mounted";
pub const APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY: &str =
    "app.lightweight_mode_on_close_to_tray";
pub const APP_SETTING_UI_LOW_TRANSPARENCY_KEY: &str = "ui.low_transparency";
pub const APP_SETTING_UI_ZOOM_FACTOR_KEY: &str = "ui.zoom_factor";
pub const APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY: &str = "ui.codex_cli_guide_dismissed";
pub const APP_SETTING_UI_THEME_KEY: &str = "ui.theme";
pub const APP_SETTING_UI_APPEARANCE_PRESET_KEY: &str = "ui.appearance_preset";
pub const APP_SETTING_UI_LOCALE_KEY: &str = "ui.locale";
pub const APP_SETTING_CODEX_PROFILE_REMOVE_REQUIRES_OPENAI_AUTH_KEY: &str =
    "codex_profile.remove_requires_openai_auth";
pub const APP_SETTING_SERVICE_ADDR_KEY: &str = "app.service_addr";
pub const APP_SETTING_AUTHOR_SPONSORS_KEY: &str = "author.sponsors";
pub const APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY: &str = "author.server_recommendations";
pub const APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY: &str = "gateway.route_strategy";
pub const APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY: &str = "gateway.free_account_max_model";
pub const APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY: &str = "gateway.model_forward_rules";
pub const APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY: &str =
    "gateway.compact_model_forward_rules";
pub const APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY: &str = "gateway.account_max_inflight";
pub const APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY: &str =
    "gateway.thread_aware_account_distribution_enabled";
pub const APP_SETTING_GATEWAY_QUOTA_GUARD_KEY: &str = "gateway.quota_guard";
pub const APP_SETTING_GATEWAY_REQUEST_COMPRESSION_ENABLED_KEY: &str =
    "gateway.request_compression_enabled";
pub const APP_SETTING_GATEWAY_ORIGINATOR_KEY: &str = "gateway.originator";
pub const APP_SETTING_GATEWAY_USER_AGENT_KEY: &str = "gateway.user_agent";
pub const APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY: &str = "gateway.user_agent_version";
pub const APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY: &str = "gateway.residency_requirement";
pub const APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY: &str = "gateway.upstream_proxy_url";
pub const APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY: &str =
    "gateway.upstream_proxy_bypass_hosts";
pub const APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY: &str =
    "gateway.upstream_stream_timeout_ms";
pub const APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY: &str =
    "gateway.upstream_total_timeout_ms";
pub const APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY: &str = "gateway.sse_keepalive_enabled";
pub const APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY: &str =
    "gateway.sse_keepalive_interval_ms";
pub const APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY: &str = "gateway.background_tasks";
pub const APP_SETTING_PLUGIN_MARKET_MODE_KEY: &str = "plugin.market_mode";
pub const APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY: &str = "plugin.market_source_url";
pub const APP_SETTING_ENV_OVERRIDES_KEY: &str = "app.env_overrides";
pub const APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY: &str = "web.auth.password_hash";
pub const APP_SETTING_WEB_AUTH_MODE_KEY: &str = "web.auth.mode";
pub const APP_SETTING_DISTRIBUTION_ENABLED_KEY: &str = "distribution.enabled";
pub const WEB_ACCESS_SESSION_COOKIE_NAME: &str = "codexmanager_web_auth";

/// 函数 `parse_bool_with_default`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn parse_bool_with_default(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// 函数 `normalize_optional_text`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn normalize_optional_text(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}
