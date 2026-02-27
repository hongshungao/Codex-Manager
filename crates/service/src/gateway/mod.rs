use crate::storage_helpers::open_storage;

mod local_validation;
mod upstream;
#[path = "request/request_helpers.rs"]
mod request_helpers;
#[path = "request/request_rewrite.rs"]
mod request_rewrite;
mod protocol_adapter;
#[path = "observability/metrics.rs"]
mod metrics;
#[path = "routing/selection.rs"]
mod selection;
#[path = "routing/failover.rs"]
mod failover;
mod model_picker;
#[path = "core/runtime_config.rs"]
mod runtime_config;
#[path = "observability/http_bridge.rs"]
mod http_bridge;
#[path = "routing/cooldown.rs"]
mod cooldown;
#[path = "auth/token_exchange.rs"]
mod token_exchange;
#[path = "auth/openai_fallback.rs"]
mod openai_fallback;
#[path = "observability/request_log.rs"]
mod request_log;
#[path = "request/request_entry.rs"]
mod request_entry;
#[path = "observability/trace_log.rs"]
mod trace_log;
#[path = "request/incoming_headers.rs"]
mod incoming_headers;
#[path = "routing/route_hint.rs"]
mod route_hint;
#[path = "request/local_count_tokens.rs"]
mod local_count_tokens;
#[path = "routing/route_quality.rs"]
mod route_quality;
#[path = "routing/request_gate.rs"]
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

// HTTP backend runtime metrics are exported via the gateway `/metrics` endpoint as well.
pub(crate) fn record_http_queue_capacity(normal_capacity: usize, stream_capacity: usize) {
    metrics::record_http_queue_capacity(normal_capacity, stream_capacity);
}

pub(crate) fn record_http_queue_enqueue(is_stream_queue: bool) {
    metrics::record_http_queue_enqueue(is_stream_queue);
}

pub(crate) fn record_http_queue_dequeue(is_stream_queue: bool) {
    metrics::record_http_queue_dequeue(is_stream_queue);
}

pub(crate) fn record_http_queue_enqueue_failure() {
    metrics::record_http_queue_enqueue_failure();
}
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
use route_hint::apply_route_strategy;
use local_count_tokens::maybe_respond_local_count_tokens;
use route_quality::record_route_quality;
use request_gate::{request_gate_lock, RequestGateAcquireError};
use runtime_config::{
    account_max_inflight_limit, request_gate_wait_timeout, trace_body_preview_max_bytes,
    upstream_cookie, fresh_upstream_client, fresh_upstream_client_for_account,
    upstream_client, upstream_client_for_account, upstream_stream_timeout, upstream_total_timeout, DEFAULT_GATEWAY_DEBUG,
    DEFAULT_MODELS_CLIENT_VERSION,
};
pub(crate) use runtime_config::front_proxy_max_body_bytes;
use upstream::proxy::proxy_validated_request;

pub(crate) fn reload_runtime_config_from_env() {
    runtime_config::reload_from_env();
    selection::reload_from_env();
    route_hint::reload_from_env();
    upstream::config::reload_from_env();
    trace_log::reload_from_env();
}

pub(crate) fn current_route_strategy() -> &'static str {
    route_hint::current_route_strategy()
}

pub(crate) fn set_route_strategy(strategy: &str) -> Result<&'static str, String> {
    let applied = route_hint::set_route_strategy(strategy)?;
    std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", applied);
    Ok(applied)
}

pub(crate) fn manual_preferred_account() -> Option<String> {
    route_hint::get_manual_preferred_account()
}

pub(crate) fn set_manual_preferred_account(account_id: &str) -> Result<(), String> {
    route_hint::set_manual_preferred_account(account_id)
}

pub(crate) fn clear_manual_preferred_account() {
    route_hint::clear_manual_preferred_account();
}

pub(crate) fn clear_manual_preferred_account_if(account_id: &str) -> bool {
    route_hint::clear_manual_preferred_account_if(account_id)
}

#[cfg(test)]
#[path = "../../tests/gateway/availability/mod.rs"]
mod availability_tests;


