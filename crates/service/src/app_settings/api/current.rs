use crate::app_settings::{list_app_settings_map, listener_bind_addr_for_mode};
use crate::initialize_storage_if_needed;
use crate::{current_web_auth_mode, distribution_enabled, web_access_password_configured};
use chrono::Local;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

use super::author_links::{
    default_author_server_recommendations, default_author_sponsors, load_author_link_items,
    serialize_author_link_items,
};
use super::{
    current_background_tasks_snapshot_value, current_env_overrides,
    current_gateway_account_max_inflight, current_gateway_compact_model_forward_rules,
    current_gateway_free_account_max_model, current_gateway_model_forward_rules,
    current_gateway_originator, current_gateway_quota_guard, current_gateway_residency_requirement,
    current_gateway_sse_keepalive_enabled, current_gateway_sse_keepalive_interval_ms,
    current_gateway_thread_aware_account_distribution_enabled,
    current_gateway_upstream_proxy_bypass_hosts, current_gateway_upstream_stream_timeout_ms,
    current_gateway_upstream_total_timeout_ms, current_gateway_user_agent,
    current_gateway_user_agent_version, current_saved_service_addr, current_service_bind_mode,
    default_gateway_originator, default_gateway_user_agent, default_gateway_user_agent_version,
    env_override_catalog_value, env_override_reserved_keys, env_override_unsupported_keys,
    normalize_optional_text, normalize_ui_appearance_preset, normalize_ui_locale,
    normalize_ui_theme, normalize_ui_zoom_factor, parse_bool_with_default,
    residency_requirement_options, save_env_overrides_value, save_persisted_app_setting,
    save_persisted_bool_setting, sync_runtime_settings_from_storage,
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
    SERVICE_BIND_MODE_ALL_INTERFACES, SERVICE_BIND_MODE_LOOPBACK, SERVICE_BIND_MODE_SETTING_KEY,
};

const DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS: &[&str] = &[
    "auto",
    "gpt-5",
    "gpt-5-codex",
    "gpt-5-codex-mini",
    "gpt-5.1",
    "gpt-5.1-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5.2",
    "gpt-5.2-codex",
    "gpt-5.3-codex",
    "gpt-5.4-mini",
    "gpt-5.4",
];

/// 函数 `normalize_service_bind_mode_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_service_bind_mode_value(raw: Option<&str>) -> &'static str {
    let Some(value) = raw else {
        return SERVICE_BIND_MODE_LOOPBACK;
    };
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "all_interfaces" | "all-interfaces" | "all" | "0.0.0.0" => SERVICE_BIND_MODE_ALL_INTERFACES,
        _ => SERVICE_BIND_MODE_LOOPBACK,
    }
}

fn setting_bool(settings: &HashMap<String, String>, key: &str, default: bool) -> bool {
    settings
        .get(key)
        .map(|value| parse_bool_with_default(value, default))
        .unwrap_or(default)
}

fn current_runtime_time_zone_value() -> Value {
    let env_tz = std::env::var("TZ")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let offset = Local::now().offset().to_string();
    let source = if env_tz.is_some() { "TZ" } else { "system" };
    let name = env_tz.unwrap_or_else(|| {
        if offset == "+00:00" {
            "UTC".to_string()
        } else {
            "Local".to_string()
        }
    });

    serde_json::json!({
        "name": name,
        "offset": offset,
        "source": source,
    })
}

/// 函数 `current_app_settings_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn current_app_settings_value(
    close_to_tray_on_close: Option<bool>,
    close_to_tray_supported: Option<bool>,
    service_listen_mode_override: Option<&str>,
) -> Result<Value, String> {
    current_app_settings_value_inner(
        close_to_tray_on_close,
        close_to_tray_supported,
        service_listen_mode_override,
        false,
    )
}

pub(super) fn current_app_settings_value_persisted(
    close_to_tray_on_close: Option<bool>,
    close_to_tray_supported: Option<bool>,
    service_listen_mode_override: Option<&str>,
) -> Result<Value, String> {
    current_app_settings_value_inner(
        close_to_tray_on_close,
        close_to_tray_supported,
        service_listen_mode_override,
        true,
    )
}
fn current_app_settings_value_inner(
    close_to_tray_on_close: Option<bool>,
    close_to_tray_supported: Option<bool>,
    service_listen_mode_override: Option<&str>,
    persist_snapshot: bool,
) -> Result<Value, String> {
    initialize_storage_if_needed()?;
    sync_runtime_settings_from_storage();
    let settings = list_app_settings_map();
    let background_tasks = current_background_tasks_snapshot_value()?;
    let runtime_time_zone = current_runtime_time_zone_value();
    let update_auto_check = setting_bool(&settings, APP_SETTING_UPDATE_AUTO_CHECK_KEY, true);
    let auto_start_enabled = setting_bool(&settings, APP_SETTING_AUTO_START_ENABLED_KEY, false);
    let remove_requires_openai_auth = setting_bool(
        &settings,
        APP_SETTING_CODEX_PROFILE_REMOVE_REQUIRES_OPENAI_AUTH_KEY,
        false,
    );
    let show_main_window_on_startup =
        setting_bool(&settings, APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY, true);
    let persisted_close_to_tray =
        setting_bool(&settings, APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY, false);
    let close_to_tray = close_to_tray_on_close.unwrap_or(persisted_close_to_tray);
    let persisted_lightweight_mode_on_close_to_tray = setting_bool(
        &settings,
        APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY,
        false,
    );
    let keep_window_ui_mounted = if settings.contains_key(APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY) {
        setting_bool(
            &settings,
            APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY,
            !cfg!(target_os = "windows"),
        )
    } else if cfg!(target_os = "windows") {
        false
    } else {
        !persisted_lightweight_mode_on_close_to_tray
    };
    let lightweight_mode_on_close_to_tray = !keep_window_ui_mounted;
    let codex_cli_guide_dismissed = setting_bool(
        &settings,
        APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY,
        false,
    );
    let low_transparency = setting_bool(&settings, APP_SETTING_UI_LOW_TRANSPARENCY_KEY, false);
    let zoom_factor = normalize_ui_zoom_factor(
        settings
            .get(APP_SETTING_UI_ZOOM_FACTOR_KEY)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(1.0),
    );
    let theme = normalize_ui_theme(settings.get(APP_SETTING_UI_THEME_KEY).map(String::as_str));
    let appearance_preset = normalize_ui_appearance_preset(
        settings
            .get(APP_SETTING_UI_APPEARANCE_PRESET_KEY)
            .map(String::as_str),
    );
    let locale = normalize_ui_locale(settings.get(APP_SETTING_UI_LOCALE_KEY).map(String::as_str));
    let service_addr = current_saved_service_addr();
    let service_listen_mode = if let Some(mode) = service_listen_mode_override {
        normalize_service_bind_mode_value(Some(mode)).to_string()
    } else if let Some(mode) = settings.get(SERVICE_BIND_MODE_SETTING_KEY) {
        normalize_service_bind_mode_value(Some(mode)).to_string()
    } else {
        current_service_bind_mode()
    };
    let route_strategy = crate::gateway::current_route_strategy().to_string();
    let free_account_max_model = current_gateway_free_account_max_model();
    let model_forward_rules = current_gateway_model_forward_rules();
    let compact_model_forward_rules = current_gateway_compact_model_forward_rules();
    let account_max_inflight = current_gateway_account_max_inflight();
    let thread_aware_account_distribution_enabled =
        current_gateway_thread_aware_account_distribution_enabled();
    let quota_guard = current_gateway_quota_guard();
    let gateway_originator = current_gateway_originator();
    let gateway_user_agent = current_gateway_user_agent();
    let gateway_user_agent_default = default_gateway_user_agent();
    let gateway_user_agent_version = current_gateway_user_agent_version();
    let gateway_originator_default = default_gateway_originator();
    let gateway_user_agent_version_default = default_gateway_user_agent_version();
    let gateway_residency_requirement = current_gateway_residency_requirement().unwrap_or_default();
    let free_account_max_model_options =
        load_free_account_max_model_options(&free_account_max_model);
    let upstream_proxy_url = crate::gateway::current_upstream_proxy_url();
    let upstream_proxy_bypass_hosts = current_gateway_upstream_proxy_bypass_hosts();
    let upstream_stream_timeout_ms = current_gateway_upstream_stream_timeout_ms();
    let upstream_total_timeout_ms = current_gateway_upstream_total_timeout_ms();
    let sse_keepalive_enabled = current_gateway_sse_keepalive_enabled();
    let sse_keepalive_interval_ms = current_gateway_sse_keepalive_interval_ms();
    let plugin_market_source_url = settings
        .get(APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY)
        .cloned()
        .unwrap_or_default();
    let author_sponsors = load_author_link_items(
        &settings,
        APP_SETTING_AUTHOR_SPONSORS_KEY,
        &default_author_sponsors(),
    );
    let author_server_recommendations = load_author_link_items(
        &settings,
        APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY,
        &default_author_server_recommendations(),
    );
    let plugin_market_mode = settings
        .get(APP_SETTING_PLUGIN_MARKET_MODE_KEY)
        .map(|value| normalize_market_mode(value))
        .unwrap_or_else(|| {
            if plugin_market_source_url.trim().is_empty() {
                "builtin"
            } else {
                "custom"
            }
        })
        .to_string();
    let background_tasks_raw = serde_json::to_string(&background_tasks)
        .map_err(|err| format!("serialize background tasks failed: {err}"))?;
    let quota_guard_raw = serde_json::to_string(&quota_guard)
        .map_err(|err| format!("serialize quota guard settings failed: {err}"))?;
    let author_sponsors_raw = serialize_author_link_items(&author_sponsors)?;
    let author_server_recommendations_raw =
        serialize_author_link_items(&author_server_recommendations)?;
    let env_overrides = current_env_overrides();
    let auth_status = crate::app_auth_status_value().unwrap_or_else(|_| {
        serde_json::json!({
            "appUsersConfigured": false,
            "appUserCount": 0,
            "activeAdminCount": 0,
        })
    });

    if persist_snapshot {
        persist_current_snapshot(
            &settings,
            update_auto_check,
            auto_start_enabled,
            show_main_window_on_startup,
            persisted_close_to_tray,
            keep_window_ui_mounted,
            lightweight_mode_on_close_to_tray,
            codex_cli_guide_dismissed,
            low_transparency,
            &theme,
            &appearance_preset,
            &locale,
            &service_addr,
            &service_listen_mode,
            &route_strategy,
            &free_account_max_model,
            &model_forward_rules,
            &compact_model_forward_rules,
            account_max_inflight,
            thread_aware_account_distribution_enabled,
            &gateway_originator,
            &gateway_user_agent_version,
            &gateway_residency_requirement,
            &quota_guard_raw,
            &plugin_market_mode,
            &plugin_market_source_url,
            &author_sponsors_raw,
            &author_server_recommendations_raw,
            upstream_proxy_url.as_deref(),
            &upstream_proxy_bypass_hosts,
            upstream_stream_timeout_ms,
            upstream_total_timeout_ms,
            sse_keepalive_enabled,
            sse_keepalive_interval_ms,
            &background_tasks_raw,
            &env_overrides,
        );
    } else {
        persist_get_snapshot_if_changed(
            &settings,
            &service_addr,
            &service_listen_mode,
            &route_strategy,
            &free_account_max_model,
            &model_forward_rules,
            thread_aware_account_distribution_enabled,
            &gateway_originator,
            &gateway_user_agent_version,
            &gateway_residency_requirement,
            upstream_proxy_url.as_deref(),
            &upstream_proxy_bypass_hosts,
            upstream_stream_timeout_ms,
            sse_keepalive_enabled,
            sse_keepalive_interval_ms,
            &background_tasks_raw,
            &env_overrides,
        );
    }

    if service_listen_mode_override.is_none() {
        if let Some(mode) = settings.get(SERVICE_BIND_MODE_SETTING_KEY) {
            let synced_addr = listener_bind_addr_for_mode(&service_addr, mode);
            std::env::set_var("CODEXMANAGER_SERVICE_ADDR", synced_addr);
        }
    }

    let mut result = serde_json::json!({
        "updateAutoCheck": update_auto_check,
        "closeToTrayOnClose": close_to_tray,
        "closeToTraySupported": close_to_tray_supported,
        "lightweightModeOnCloseToTray": lightweight_mode_on_close_to_tray,
        "codexCliGuideDismissed": codex_cli_guide_dismissed,
        "lowTransparency": low_transparency,
        "theme": theme,
        "appearancePreset": appearance_preset,
        "locale": locale,
        "localeOptions": ["zh-CN", "en", "ru", "ko"],
        "serviceAddr": service_addr,
        "serviceListenMode": service_listen_mode,
        "serviceListenModeOptions": [
            SERVICE_BIND_MODE_LOOPBACK,
            SERVICE_BIND_MODE_ALL_INTERFACES
        ],
        "routeStrategy": route_strategy,
        "routeStrategyOptions": ["ordered", "balanced"],
        "freeAccountMaxModel": free_account_max_model,
        "modelForwardRules": model_forward_rules,
        "compactModelForwardRules": compact_model_forward_rules,
        "accountMaxInflight": account_max_inflight,
        "quotaGuard": quota_guard,
        "freeAccountMaxModelOptions": free_account_max_model_options,
        "gatewayOriginator": gateway_originator,
        "gatewayOriginatorDefault": gateway_originator_default,
        "gatewayUserAgentVersion": gateway_user_agent_version,
        "gatewayUserAgentVersionDefault": gateway_user_agent_version_default,
        "gatewayResidencyRequirement": gateway_residency_requirement,
        "pluginMarketMode": plugin_market_mode,
        "pluginMarketSourceUrl": plugin_market_source_url,
        "authorSponsors": author_sponsors,
        "authorServerRecommendations": author_server_recommendations,
        "gatewayResidencyRequirementOptions": residency_requirement_options(),
        "upstreamProxyUrl": upstream_proxy_url.unwrap_or_default(),
        "upstreamStreamTimeoutMs": upstream_stream_timeout_ms,
        "upstreamTotalTimeoutMs": upstream_total_timeout_ms,
        "sseKeepaliveIntervalMs": sse_keepalive_interval_ms,
        "backgroundTasks": background_tasks,
        "envOverrides": env_overrides,
        "envOverrideCatalog": env_override_catalog_value(),
        "envOverrideReservedKeys": env_override_reserved_keys(),
        "envOverrideUnsupportedKeys": env_override_unsupported_keys(),
        "webAccessPasswordConfigured": web_access_password_configured(),
    });
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "removeRequiresOpenaiAuth".to_string(),
            remove_requires_openai_auth.into(),
        );
        object.insert("zoomFactor".to_string(), zoom_factor.into());
        object.insert("gatewayUserAgent".to_string(), gateway_user_agent.into());
        object.insert(
            "gatewayUserAgentDefault".to_string(),
            gateway_user_agent_default.into(),
        );
    }
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "keepWindowUiMounted".to_string(),
            serde_json::json!(keep_window_ui_mounted),
        );
        object.insert(
            "showMainWindowOnStartup".to_string(),
            show_main_window_on_startup.into(),
        );
        object.insert("autoStartEnabled".to_string(), auto_start_enabled.into());
        object.insert("autoStartSupported".to_string(), false.into());
        object.insert(
            "threadAwareAccountDistributionEnabled".to_string(),
            thread_aware_account_distribution_enabled.into(),
        );
        object.insert(
            "upstreamProxyBypassHosts".to_string(),
            upstream_proxy_bypass_hosts.into(),
        );
        object.insert(
            "sseKeepaliveEnabled".to_string(),
            sse_keepalive_enabled.into(),
        );
        object.insert("runtimeTimeZone".to_string(), runtime_time_zone);
        object.insert("webAuthMode".to_string(), current_web_auth_mode().into());
        object.insert(
            "webAuthModeOptions".to_string(),
            serde_json::json!(["none", "password", "accounts"]),
        );
        object.insert(
            "distributionEnabled".to_string(),
            distribution_enabled().into(),
        );
        object.insert(
            "billingModeLock".to_string(),
            auth_status
                .get("billingModeLock")
                .cloned()
                .unwrap_or_else(|| {
                    serde_json::json!({
                        "accountModeLocked": false,
                        "distributionLocked": false,
                        "reasons": []
                    })
                }),
        );
        object.insert(
            "appUsersConfigured".to_string(),
            auth_status
                .get("appUsersConfigured")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                .into(),
        );
        object.insert(
            "appUserCount".to_string(),
            auth_status
                .get("appUserCount")
                .and_then(|value| value.as_i64())
                .unwrap_or(0)
                .into(),
        );
    }
    Ok(result)
}

pub(super) fn current_author_content_value() -> Result<Value, String> {
    initialize_storage_if_needed()?;
    sync_runtime_settings_from_storage();
    let settings = list_app_settings_map();
    let author_sponsors = load_author_link_items(
        &settings,
        APP_SETTING_AUTHOR_SPONSORS_KEY,
        &default_author_sponsors(),
    );
    let author_server_recommendations = load_author_link_items(
        &settings,
        APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY,
        &default_author_server_recommendations(),
    );
    Ok(serde_json::json!({
        "authorSponsors": author_sponsors,
        "authorServerRecommendations": author_server_recommendations,
    }))
}

/// 函数 `load_free_account_max_model_options`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - current: 参数 current
///
/// # 返回
/// 返回函数执行结果
fn load_free_account_max_model_options(current: &str) -> Vec<String> {
    let catalog = crate::storage_helpers::open_storage()
        .and_then(|storage| storage.list_api_models_v2().ok())
        .unwrap_or_default();
    let known_current_is_non_text = catalog.iter().any(|model| {
        model.slug.eq_ignore_ascii_case(current.trim())
            && !crate::models_v2::supports_text_generation(model)
    });
    let cached = catalog
        .into_iter()
        .filter(crate::models_v2::supports_text_generation)
        .map(|model| model.slug)
        .filter(|slug| slug.starts_with("gpt-"))
        .collect::<Vec<_>>();
    collect_free_account_max_model_options(
        if known_current_is_non_text {
            "auto"
        } else {
            current
        },
        &cached,
    )
}

/// 函数 `collect_free_account_max_model_options`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - current: 参数 current
/// - cached: 参数 cached
///
/// # 返回
/// 返回函数执行结果
fn collect_free_account_max_model_options(current: &str, cached: &[String]) -> Vec<String> {
    let mut items = vec!["auto".to_string()];
    for slug in cached
        .iter()
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|slug| is_free_account_max_model_option(slug))
    {
        if !items.iter().any(|item| item == &slug) {
            items.push(slug);
        }
    }

    if items.len() == 1 {
        items = DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS
            .iter()
            .map(|item| (*item).to_string())
            .collect();
    }

    let normalized_current = current.trim().to_ascii_lowercase();
    if (normalized_current == "auto" || is_free_account_max_model_option(&normalized_current))
        && !items.iter().any(|item| item == &normalized_current)
    {
        items.push(normalized_current);
    }

    items
}

/// 函数 `is_free_account_max_model_option`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - slug: 参数 slug
///
/// # 返回
/// 返回函数执行结果
fn is_free_account_max_model_option(slug: &str) -> bool {
    let normalized = slug.trim().to_ascii_lowercase();
    !normalized.is_empty() && normalized.starts_with("gpt-") && normalized != "gpt-5.4-pro"
}

/// 函数 `persist_current_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - update_auto_check: 参数 update_auto_check
/// - persisted_close_to_tray: 参数 persisted_close_to_tray
/// - lightweight_mode_on_close_to_tray: 参数 lightweight_mode_on_close_to_tray
/// - low_transparency: 参数 low_transparency
/// - theme: 参数 theme
/// - appearance_preset: 参数 appearance_preset
/// - service_addr: 参数 service_addr
/// - service_listen_mode: 参数 service_listen_mode
/// - route_strategy: 参数 route_strategy
/// - free_account_max_model: 参数 free_account_max_model
/// - account_max_inflight: 参数 account_max_inflight
/// - gateway_originator: 参数 gateway_originator
/// - gateway_user_agent_version: 参数 gateway_user_agent_version
/// - gateway_residency_requirement: 参数 gateway_residency_requirement
/// - plugin_market_mode: 参数 plugin_market_mode
/// - plugin_market_source_url: 参数 plugin_market_source_url
/// - author_sponsors_raw: 参数 author_sponsors_raw
/// - author_server_recommendations_raw: 参数 author_server_recommendations_raw
/// - upstream_proxy_url: 参数 upstream_proxy_url
/// - upstream_stream_timeout_ms: 参数 upstream_stream_timeout_ms
/// - upstream_total_timeout_ms: 参数 upstream_total_timeout_ms
/// - sse_keepalive_interval_ms: 参数 sse_keepalive_interval_ms
/// - background_tasks_raw: 参数 background_tasks_raw
/// - env_overrides: 参数 env_overrides
///
/// # 返回
/// 无
fn persist_current_snapshot(
    settings: &HashMap<String, String>,
    update_auto_check: bool,
    auto_start_enabled: bool,
    show_main_window_on_startup: bool,
    persisted_close_to_tray: bool,
    keep_window_ui_mounted: bool,
    lightweight_mode_on_close_to_tray: bool,
    codex_cli_guide_dismissed: bool,
    low_transparency: bool,
    theme: &str,
    appearance_preset: &str,
    locale: &str,
    service_addr: &str,
    service_listen_mode: &str,
    route_strategy: &str,
    free_account_max_model: &str,
    model_forward_rules: &str,
    compact_model_forward_rules: &str,
    account_max_inflight: usize,
    thread_aware_account_distribution_enabled: bool,
    gateway_originator: &str,
    gateway_user_agent_version: &str,
    gateway_residency_requirement: &str,
    quota_guard_raw: &str,
    plugin_market_mode: &str,
    plugin_market_source_url: &str,
    author_sponsors_raw: &str,
    author_server_recommendations_raw: &str,
    upstream_proxy_url: Option<&str>,
    upstream_proxy_bypass_hosts: &str,
    upstream_stream_timeout_ms: u64,
    upstream_total_timeout_ms: u64,
    sse_keepalive_enabled: bool,
    sse_keepalive_interval_ms: u64,
    background_tasks_raw: &str,
    env_overrides: &BTreeMap<String, String>,
) {
    let _ = save_persisted_bool_setting(APP_SETTING_UPDATE_AUTO_CHECK_KEY, update_auto_check);
    let _ = save_persisted_bool_setting(APP_SETTING_AUTO_START_ENABLED_KEY, auto_start_enabled);
    let _ = save_persisted_bool_setting(
        APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY,
        show_main_window_on_startup,
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
        persisted_close_to_tray,
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY,
        keep_window_ui_mounted,
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY,
        lightweight_mode_on_close_to_tray,
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY,
        codex_cli_guide_dismissed,
    );
    let _ = save_persisted_bool_setting(APP_SETTING_UI_LOW_TRANSPARENCY_KEY, low_transparency);
    let _ = save_persisted_app_setting(APP_SETTING_UI_THEME_KEY, Some(theme));
    let _ = save_persisted_app_setting(
        APP_SETTING_UI_APPEARANCE_PRESET_KEY,
        Some(appearance_preset),
    );
    let _ = save_persisted_app_setting(APP_SETTING_UI_LOCALE_KEY, Some(locale));
    let _ = save_persisted_app_setting(APP_SETTING_SERVICE_ADDR_KEY, Some(service_addr));
    let _ = save_persisted_app_setting(SERVICE_BIND_MODE_SETTING_KEY, Some(service_listen_mode));
    let _ =
        save_persisted_app_setting(APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY, Some(route_strategy));
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
        Some(free_account_max_model),
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
        if model_forward_rules.trim().is_empty() {
            None
        } else {
            Some(model_forward_rules)
        },
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
        if compact_model_forward_rules.trim().is_empty() {
            None
        } else {
            Some(compact_model_forward_rules)
        },
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY,
        Some(&account_max_inflight.to_string()),
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
        thread_aware_account_distribution_enabled,
    );
    let _ =
        save_persisted_app_setting(APP_SETTING_GATEWAY_ORIGINATOR_KEY, Some(gateway_originator));
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY,
        Some(gateway_user_agent_version),
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY,
        if gateway_residency_requirement.trim().is_empty() {
            None
        } else {
            Some(gateway_residency_requirement)
        },
    );
    let _ = save_persisted_app_setting(APP_SETTING_GATEWAY_QUOTA_GUARD_KEY, Some(quota_guard_raw));
    let _ = save_persisted_app_setting(
        APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY,
        if plugin_market_source_url.trim().is_empty() {
            None
        } else {
            Some(plugin_market_source_url)
        },
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_PLUGIN_MARKET_MODE_KEY,
        if plugin_market_mode.trim().is_empty() {
            None
        } else {
            Some(plugin_market_mode)
        },
    );
    let _ = save_persisted_app_setting(APP_SETTING_AUTHOR_SPONSORS_KEY, Some(author_sponsors_raw));
    let _ = save_persisted_app_setting(
        APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY,
        Some(author_server_recommendations_raw),
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
        upstream_proxy_url,
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
        if upstream_proxy_bypass_hosts.trim().is_empty() {
            None
        } else {
            Some(upstream_proxy_bypass_hosts)
        },
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
        Some(&upstream_stream_timeout_ms.to_string()),
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY,
        Some(&upstream_total_timeout_ms.to_string()),
    );
    if !crate::gateway::sse_keepalive_enabled_is_env_overridden()
        || !settings.contains_key(APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY)
    {
        let _ = save_persisted_bool_setting(
            APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY,
            sse_keepalive_enabled,
        );
    }
    if !crate::gateway::sse_keepalive_interval_is_env_overridden()
        || !settings.contains_key(APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY)
    {
        let _ = save_persisted_app_setting(
            APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
            Some(&sse_keepalive_interval_ms.to_string()),
        );
    }
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
        Some(background_tasks_raw),
    );
    let _ = save_env_overrides_value(env_overrides);
}

fn persisted_text_value(value: Option<&str>) -> String {
    normalize_optional_text(value).unwrap_or_default()
}

fn save_persisted_bool_setting_if_changed(
    settings: &HashMap<String, String>,
    key: &str,
    value: bool,
) {
    let text = if value { "1" } else { "0" };
    if settings.get(key).map(String::as_str) == Some(text) {
        return;
    }
    let _ = save_persisted_bool_setting(key, value);
}

fn save_app_setting_if_changed(settings: &HashMap<String, String>, key: &str, value: Option<&str>) {
    let text = persisted_text_value(value);
    if settings.get(key).map(String::as_str) == Some(text.as_str()) {
        return;
    }
    let _ = save_persisted_app_setting(key, if text.is_empty() { None } else { Some(&text) });
}

fn save_env_overrides_if_changed(
    settings: &HashMap<String, String>,
    env_overrides: &BTreeMap<String, String>,
) {
    let Ok(raw) = serde_json::to_string(env_overrides) else {
        return;
    };
    if settings
        .get(APP_SETTING_ENV_OVERRIDES_KEY)
        .map(String::as_str)
        == Some(raw.as_str())
    {
        return;
    }
    let _ = save_env_overrides_value(env_overrides);
}

fn persist_get_snapshot_if_changed(
    settings: &HashMap<String, String>,
    service_addr: &str,
    service_listen_mode: &str,
    route_strategy: &str,
    free_account_max_model: &str,
    model_forward_rules: &str,
    thread_aware_account_distribution_enabled: bool,
    gateway_originator: &str,
    gateway_user_agent_version: &str,
    gateway_residency_requirement: &str,
    upstream_proxy_url: Option<&str>,
    upstream_proxy_bypass_hosts: &str,
    upstream_stream_timeout_ms: u64,
    sse_keepalive_enabled: bool,
    sse_keepalive_interval_ms: u64,
    background_tasks_raw: &str,
    env_overrides: &BTreeMap<String, String>,
) {
    save_app_setting_if_changed(settings, APP_SETTING_SERVICE_ADDR_KEY, Some(service_addr));
    save_app_setting_if_changed(
        settings,
        SERVICE_BIND_MODE_SETTING_KEY,
        Some(service_listen_mode),
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
        Some(route_strategy),
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
        Some(free_account_max_model),
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
        if model_forward_rules.trim().is_empty() {
            None
        } else {
            Some(model_forward_rules)
        },
    );
    save_persisted_bool_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
        thread_aware_account_distribution_enabled,
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_ORIGINATOR_KEY,
        Some(gateway_originator),
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY,
        Some(gateway_user_agent_version),
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY,
        if gateway_residency_requirement.trim().is_empty() {
            None
        } else {
            Some(gateway_residency_requirement)
        },
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
        upstream_proxy_url,
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
        if upstream_proxy_bypass_hosts.trim().is_empty() {
            None
        } else {
            Some(upstream_proxy_bypass_hosts)
        },
    );
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
        Some(&upstream_stream_timeout_ms.to_string()),
    );
    if !crate::gateway::sse_keepalive_enabled_is_env_overridden()
        || !settings.contains_key(APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY)
    {
        save_persisted_bool_setting_if_changed(
            settings,
            APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY,
            sse_keepalive_enabled,
        );
    }
    if !crate::gateway::sse_keepalive_interval_is_env_overridden()
        || !settings.contains_key(APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY)
    {
        save_app_setting_if_changed(
            settings,
            APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
            Some(&sse_keepalive_interval_ms.to_string()),
        );
    }
    save_app_setting_if_changed(
        settings,
        APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
        Some(background_tasks_raw),
    );
    save_env_overrides_if_changed(settings, env_overrides);
}
/// 函数 `normalize_market_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_market_mode(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "private" => "private",
        "custom" => "custom",
        _ => "builtin",
    }
}

#[cfg(test)]
#[path = "current_tests.rs"]
mod tests;
