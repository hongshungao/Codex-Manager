use crate::storage_helpers::open_storage;

mod local_validation;
mod upstream;
mod request_helpers;
mod request_rewrite;
mod protocol_adapter;
mod metrics;
mod selection;
mod failover;
mod model_picker;
mod runtime_config;
mod http_bridge;
mod cooldown;
mod token_exchange;
mod openai_fallback;
mod request_log;
mod request_entry;
mod trace_log;
mod incoming_headers;
mod route_hint;
mod local_count_tokens;
mod route_quality;
mod request_gate;

pub(super) use request_helpers::{
    is_html_content_type, is_upstream_challenge_response, normalize_models_path,
    parse_request_metadata,
};
#[cfg(test)]
use request_helpers::{should_drop_incoming_header, should_drop_incoming_header_for_failover};
use request_rewrite::{apply_request_overrides, compute_upstream_url};
use protocol_adapter::{
    adapt_request_for_protocol, adapt_upstream_response, build_anthropic_error_body,
    ResponseAdapter,
};
use upstream::config::{
    is_openai_api_base, resolve_upstream_base_url, resolve_upstream_fallback_base_url,
    should_try_openai_fallback, should_try_openai_fallback_by_status,
};
#[cfg(test)]
use upstream::config::normalize_upstream_base_url;
#[cfg(test)]
use upstream::header_profile::{build_codex_upstream_headers, CodexUpstreamHeaderInput};
use metrics::{
    account_inflight_count, acquire_account_inflight, begin_gateway_request,
    record_gateway_cooldown_mark, record_gateway_failover_attempt,
    record_gateway_request_outcome, AccountInFlightGuard,
};
pub(crate) use metrics::{
    begin_rpc_request, duration_to_millis, gateway_metrics_prometheus,
    record_usage_refresh_outcome,
};
use selection::collect_gateway_candidates;
use upstream::candidates::prepare_gateway_candidates;
use failover::should_failover_from_cached_snapshot;
#[cfg(test)]
pub(super) use failover::should_failover_after_refresh;
pub(crate) use model_picker::fetch_models_for_picker;
use http_bridge::respond_with_upstream;
use cooldown::{
    clear_account_cooldown, is_account_in_cooldown, mark_account_cooldown,
    mark_account_cooldown_for_status, CooldownReason,
};
#[cfg(test)]
use cooldown::cooldown_reason_for_status;
#[cfg(test)]
use token_exchange::account_token_exchange_lock;
use token_exchange::resolve_openai_bearer_token;
use openai_fallback::try_openai_fallback;
use request_log::write_request_log;
pub(crate) use request_entry::handle_gateway_request;
pub(super) use incoming_headers::IncomingHeaderSnapshot;
use route_hint::{preferred_route_account, remember_success_route_account};
use local_count_tokens::maybe_respond_local_count_tokens;
use route_quality::{record_route_quality, route_quality_penalty};
use request_gate::{request_gate_lock, RequestGateAcquireError};
use runtime_config::{
    account_max_inflight_limit, request_gate_wait_timeout, trace_body_preview_max_bytes,
    upstream_cookie,
    upstream_client, upstream_stream_timeout, upstream_total_timeout, DEFAULT_GATEWAY_DEBUG,
    DEFAULT_MODELS_CLIENT_VERSION,
};
pub(crate) use runtime_config::front_proxy_max_body_bytes;
use upstream::proxy::proxy_validated_request;

pub(crate) fn reload_runtime_config_from_env() {
    runtime_config::reload_from_env();
    selection::reload_from_env();
    upstream::config::reload_from_env();
    trace_log::reload_from_env();
}

#[cfg(test)]
#[path = "../../tests/gateway/availability/mod.rs"]
mod availability_tests;


