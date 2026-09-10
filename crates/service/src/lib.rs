use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

pub const RPC_BODY_LIMIT_BYTES: usize = 24 * 1024 * 1024;

mod account;
mod account_identity;
mod agent_identity;
mod aggregate_api;
mod apikey;
pub(crate) mod app_settings;
mod auth;
mod codex_model_catalog;
mod codex_profile;
mod codex_runtime;
mod codex_skill_repositories;
mod codex_skills;
mod codex_skills_marketplace;
mod dashboard;
mod errors;
mod gateway;
mod http;
mod lifecycle;
mod logging;
mod model_groups;
mod models_v2;
mod plugin;
mod proxy_registry;
mod quota;
mod requestlog;
mod rpc_actor;
mod rpc_dispatch;
mod runtime;
mod startup_snapshot;
mod storage;
mod time_bounds;
mod usage;

pub(crate) use account::availability as account_availability;
pub(crate) use account::cleanup as account_cleanup;
pub(crate) use account::delete as account_delete;
pub(crate) use account::delete_many as account_delete_many;
pub(crate) use account::export as account_export;
pub(crate) use account::group as account_group;
pub(crate) use account::import as account_import;
pub(crate) use account::list as account_list;
pub(crate) use account::models as account_models;
pub(crate) use account::plan as account_plan;
pub(crate) use account::proxy as account_proxy;
pub(crate) use account::proxy_testing::presets::proxy_test_presets;
pub(crate) use account::status as account_status;
pub(crate) use account::test as account_test;
pub(crate) use account::update as account_update;
pub(crate) use account::warmup as account_warmup;
pub(crate) use aggregate_api::{
    associate_aggregate_api_models, create_aggregate_api, delete_aggregate_api,
    fetch_aggregate_api_models, list_aggregate_apis, read_aggregate_api_secret,
    refresh_aggregate_api_balance, test_aggregate_api_connection, update_aggregate_api,
};
pub(crate) use apikey::create as apikey_create;
pub(crate) use apikey::delete as apikey_delete;
pub(crate) use apikey::disable as apikey_disable;
pub(crate) use apikey::enable as apikey_enable;
pub(crate) use apikey::list as apikey_list;
pub(crate) use apikey::profile as apikey_profile;
pub(crate) use apikey::read_secret as apikey_read_secret;
pub(crate) use apikey::update_model as apikey_update_model;
pub(crate) use apikey::usage_stats as apikey_usage_stats;
pub(crate) use auth::account as auth_account;
pub(crate) use auth::app_manager::distribution_enabled_for_storage;
pub(crate) use auth::callback as auth_callback;
pub(crate) use auth::login as auth_login;
pub(crate) use auth::tokens as auth_tokens;
pub(crate) use errors as error_codes;
pub(crate) use model_groups::{
    allowed_model_slugs_for_api_key, delete_model_group, read_model_groups,
    resolve_api_key_model_group_access, set_model_group_models, set_model_group_users,
    upsert_model_group,
};
pub(crate) use proxy_registry::{
    cancel_proxy_test_job, create_proxy_profile, delete_proxy_profile,
    get_proxy_profile_diagnostics_history, get_proxy_profile_latency_test_history,
    get_proxy_profile_speed_test_history, get_proxy_test_job, list_proxy_profiles,
    test_proxy_profile, test_proxy_profile_cloudflare_style_speed, test_proxy_profile_latency,
    test_proxy_profile_speed, update_proxy_profile,
};

pub(crate) use requestlog::clear as requestlog_clear;
pub(crate) use requestlog::list as requestlog_list;
pub(crate) use requestlog::summary as requestlog_summary;
pub(crate) use requestlog::today_summary as requestlog_today_summary;
pub(crate) use runtime::lock_utils;
pub use runtime::process_env;
pub(crate) use runtime::reasoning_effort;
pub(crate) use storage::helpers as storage_helpers;
pub(crate) use usage::account_meta as usage_account_meta;
pub(crate) use usage::aggregate as usage_aggregate;
pub(crate) use usage::http as usage_http;
pub(crate) use usage::keepalive as usage_keepalive;
pub(crate) use usage::list as usage_list;
pub(crate) use usage::read as usage_read;
pub(crate) use usage::refresh as usage_refresh;
pub(crate) use usage::reset_credits as usage_reset_credits;
pub(crate) use usage::scheduler as usage_scheduler;
pub(crate) use usage::snapshot_store as usage_snapshot_store;
pub(crate) use usage::token_refresh as usage_token_refresh;

pub use account_test::{set_account_test_event_handler, AccountTestEvent};
pub use app_settings::{
    app_settings_get, app_settings_get_with_overrides, app_settings_set, author_content_get,
    bind_all_interfaces_enabled, bind_all_interfaces_enabled_for_mode,
    current_auto_start_enabled_setting, current_close_to_tray_on_close_setting,
    current_codex_cli_guide_dismissed, current_gateway_account_max_inflight,
    current_gateway_free_account_max_model, current_gateway_model_forward_rules,
    current_gateway_originator, current_gateway_request_compression_enabled,
    current_gateway_residency_requirement, current_gateway_sse_keepalive_enabled,
    current_gateway_sse_keepalive_interval_ms,
    current_gateway_thread_aware_account_distribution_enabled,
    current_gateway_upstream_proxy_bypass_hosts, current_gateway_upstream_stream_timeout_ms,
    current_gateway_upstream_total_timeout_ms, current_gateway_user_agent,
    current_gateway_user_agent_version, current_keep_window_ui_mounted_setting,
    current_lightweight_mode_on_close_to_tray_setting, current_saved_service_addr,
    current_service_bind_mode, current_show_main_window_on_startup_setting,
    current_ui_appearance_preset, current_ui_low_transparency_enabled, current_ui_theme,
    current_update_auto_check_enabled, default_gateway_originator, default_gateway_user_agent,
    default_gateway_user_agent_version, default_listener_bind_addr, default_web_listener_addr,
    fetch_codex_latest_version, listener_bind_addr, listener_bind_addr_for_mode,
    residency_requirement_options, set_auto_start_enabled_setting,
    set_close_to_tray_on_close_setting, set_codex_cli_guide_dismissed,
    set_gateway_account_max_inflight, set_gateway_background_tasks,
    set_gateway_free_account_max_model, set_gateway_model_forward_rules, set_gateway_originator,
    set_gateway_request_compression_enabled, set_gateway_residency_requirement,
    set_gateway_route_strategy, set_gateway_sse_keepalive_enabled,
    set_gateway_sse_keepalive_interval_ms, set_gateway_thread_aware_account_distribution_enabled,
    set_gateway_upstream_proxy_bypass_hosts, set_gateway_upstream_proxy_url,
    set_gateway_upstream_stream_timeout_ms, set_gateway_upstream_total_timeout_ms,
    set_gateway_user_agent, set_gateway_user_agent_version, set_keep_window_ui_mounted_setting,
    set_lightweight_mode_on_close_to_tray_setting, set_saved_service_addr, set_service_bind_mode,
    set_show_main_window_on_startup_setting, set_ui_appearance_preset,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
    sync_runtime_settings_from_storage, BackgroundTasksInput, APP_SETTING_AUTO_START_ENABLED_KEY,
    APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
    APP_SETTING_CODEX_PROFILE_REMOVE_REQUIRES_OPENAI_AUTH_KEY,
    APP_SETTING_DISTRIBUTION_ENABLED_KEY, APP_SETTING_ENV_OVERRIDES_KEY,
    APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY, APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
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
    APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, APP_SETTING_SERVICE_ADDR_KEY,
    APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY, APP_SETTING_UI_APPEARANCE_PRESET_KEY,
    APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY, APP_SETTING_UI_LOW_TRANSPARENCY_KEY,
    APP_SETTING_UI_THEME_KEY, APP_SETTING_UPDATE_AUTO_CHECK_KEY,
    APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY, APP_SETTING_WEB_AUTH_MODE_KEY, DEFAULT_ADDR,
    DEFAULT_BIND_ADDR, DEFAULT_WEB_ADDR, DEFAULT_WEB_BIND_ADDR, SERVICE_BIND_MODE_ALL_INTERFACES,
    SERVICE_BIND_MODE_LOOPBACK, SERVICE_BIND_MODE_SETTING_KEY, WEB_ACCESS_SESSION_COOKIE_NAME,
};
pub use auth::{
    api_key_belongs_to_user, app_auth_status_value, app_session_result, billing_mode_lock_status,
    bootstrap_app_admin, build_web_access_session_token, change_app_user_password, create_app_user,
    current_web_access_password_hash, current_web_auth_mode, delete_app_user, distribution_enabled,
    list_api_key_ids_for_user, list_api_key_owners, list_app_users, login_app_user,
    logout_app_user_session, record_request_charge_v2, resolve_app_user_session, set_api_key_owner,
    set_distribution_enabled, set_web_access_password, set_web_auth_mode, update_app_user,
    update_app_user_profile, verify_web_access_password, wallet_precheck_for_api_key,
    wallet_set_available_credit, wallet_top_up, web_access_auth_status_value,
    web_access_password_configured, web_auth_status_value, ApiKeyOwnerResult, AppLoginResult,
    AppSessionResult, AppSessionUserResult, AppUserCreateInput, AppUserPublicResult,
    AppUserUpdateInput, AppWalletResult, BillingModeLockResult,
};
pub use auth::{rpc_auth_token, rpc_auth_token_matches};
pub use lifecycle::bootstrap::{initialize_storage_if_needed, portable};
pub use lifecycle::shutdown::{clear_shutdown_flag, request_shutdown, shutdown_requested};
pub use lifecycle::startup::{start_one_shot_server, start_server, ServerHandle};
pub use logging::init_logging;
pub use rpc_actor::{RpcActor, ROLE_ADMIN, ROLE_MEMBER, ROLE_SYSTEM_ADMIN};
pub use usage::tray_summary::{read_tray_usage_reset_summary, TrayUsageResetSummary};
pub use usage_refresh::{set_usage_refresh_completed_handler, UsageRefreshCompletedEvent};

/// 函数 `test_env_guard`
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
#[cfg(test)]
pub(crate) fn test_env_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_ENV_MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    TEST_ENV_MUTEX
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 函数 `handle_request`
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
#[cfg(test)]
pub(crate) fn handle_request(req: JsonRpcRequest) -> JsonRpcMessage {
    rpc_dispatch::handle_request(req)
}

pub(crate) fn handle_request_with_actor(req: JsonRpcRequest, actor: RpcActor) -> JsonRpcMessage {
    rpc_dispatch::handle_request_with_actor(req, actor)
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
