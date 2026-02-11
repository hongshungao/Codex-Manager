use crate::storage_helpers::open_storage;

mod local_validation;
mod upstream_proxy;
mod upstream_transport;
mod upstream_retry;
mod upstream_stateless_retry;
mod upstream_candidates;
mod upstream_openai_base;
mod upstream_fallback_branch;
mod upstream_outcome;
mod upstream_precheck;
mod upstream_primary_attempt;
mod upstream_primary_flow;
mod upstream_candidate_flow;
mod upstream_postprocess;
mod request_helpers;
mod request_rewrite;
mod metrics;
mod selection;
mod failover;
mod model_picker;
mod upstream_config;
mod runtime_config;
mod http_bridge;
mod cooldown;
mod token_exchange;
mod openai_fallback;
mod request_log;
mod request_entry;

pub(super) use request_helpers::{
    extract_request_model, extract_request_reasoning_effort, is_html_content_type,
    is_upstream_challenge_response, normalize_models_path, should_drop_incoming_header,
    should_drop_incoming_header_for_failover,
};
use request_rewrite::{apply_request_overrides, compute_upstream_url};
use upstream_config::{
    is_openai_api_base,
    resolve_upstream_base_url, resolve_upstream_fallback_base_url, should_try_openai_fallback,
    should_try_openai_fallback_by_status,
};
#[cfg(test)]
use upstream_config::normalize_upstream_base_url;
pub(crate) use metrics::{
    AccountInFlightGuard, acquire_account_inflight,
    begin_gateway_request, gateway_metrics_prometheus,
    account_inflight_count, record_gateway_cooldown_mark, record_gateway_failover_attempt,
};
pub(crate) use selection::{collect_gateway_candidates, rotate_candidates_for_fairness};
pub(crate) use upstream_candidates::prepare_gateway_candidates;
pub(crate) use failover::should_failover_after_refresh;
pub(crate) use model_picker::fetch_models_for_picker;
use http_bridge::{extract_platform_key, respond_with_upstream};
use cooldown::{
    CooldownReason, clear_account_cooldown, is_account_in_cooldown, mark_account_cooldown,
    mark_account_cooldown_for_status,
};
#[cfg(test)]
use cooldown::cooldown_reason_for_status;
use token_exchange::resolve_openai_bearer_token;
#[cfg(test)]
use token_exchange::account_token_exchange_lock;
use openai_fallback::try_openai_fallback;
use request_log::write_request_log;
pub(crate) use request_entry::handle_gateway_request;
pub(crate) use runtime_config::{
    account_max_inflight_limit, upstream_client, DEFAULT_GATEWAY_DEBUG,
    DEFAULT_MODELS_CLIENT_VERSION,
};

#[cfg(test)]
mod availability_tests;



















