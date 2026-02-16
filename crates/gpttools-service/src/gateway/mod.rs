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
mod route_hint;
mod local_count_tokens;
mod route_quality;

pub(super) use request_helpers::{
    extract_request_model, extract_request_reasoning_effort, extract_request_stream,
    is_html_content_type, is_upstream_challenge_response, normalize_models_path,
    summarize_request_shape,
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
    record_gateway_cooldown_mark, record_gateway_failover_attempt, AccountInFlightGuard,
};
pub(crate) use metrics::gateway_metrics_prometheus;
use selection::{collect_gateway_candidates, rotate_candidates_for_fairness};
use upstream::candidates::prepare_gateway_candidates;
use failover::should_failover_after_refresh;
pub(crate) use model_picker::fetch_models_for_picker;
use http_bridge::{extract_platform_key, respond_with_upstream};
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
use route_hint::{preferred_route_account, remember_success_route_account};
use local_count_tokens::maybe_respond_local_count_tokens;
use route_quality::{record_route_quality, route_quality_penalty};
use runtime_config::{
    account_max_inflight_limit, upstream_client, DEFAULT_GATEWAY_DEBUG,
    DEFAULT_MODELS_CLIENT_VERSION,
};
use upstream::proxy::proxy_validated_request;

#[cfg(test)]
#[path = "../../tests/gateway/availability/mod.rs"]
mod availability_tests;


