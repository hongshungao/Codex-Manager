use codexmanager_core::storage::Account;
use reqwest::StatusCode;
use std::time::{Duration, Instant};
use tiny_http::Request;

use super::transport::send_upstream_request;

pub(super) enum StatelessRetryResult {
    NotTriggered,
    Upstream(reqwest::blocking::Response),
    Terminal { status_code: u16, message: String },
}

fn should_trigger_stateless_retry(
    status: u16,
    strip_session_affinity: bool,
    disable_challenge_stateless_retry: bool,
) -> bool {
    if strip_session_affinity {
        return !disable_challenge_stateless_retry && status == 403;
    }
    if disable_challenge_stateless_retry {
        return matches!(status, 401 | 404);
    }
    matches!(status, 401 | 403 | 404)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retry_stateless_then_optional_alt(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    primary_url: &str,
    alt_url: Option<&str>,
    request_deadline: Option<Instant>,
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &[u8],
    is_stream: bool,
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    status: StatusCode,
    debug: bool,
    disable_challenge_stateless_retry: bool,
) -> StatelessRetryResult {
    if super::deadline::is_expired(request_deadline) {
        return StatelessRetryResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    if !should_trigger_stateless_retry(
        status.as_u16(),
        strip_session_affinity,
        disable_challenge_stateless_retry,
    ) {
        return StatelessRetryResult::NotTriggered;
    }
    if debug {
        eprintln!(
            "gateway upstream stateless retry: account_id={}, status={}",
            account.id, status
        );
    }
    if status.as_u16() == 403 {
        if !super::backoff::sleep_with_exponential_jitter(
            Duration::from_millis(120),
            Duration::from_millis(900),
            1,
            request_deadline,
        ) {
            return StatelessRetryResult::Terminal {
                status_code: 504,
                message: "upstream total timeout exceeded".to_string(),
            };
        }
    }
    let mut response = match send_upstream_request(
        client,
        method,
        primary_url,
        request_deadline,
        request,
        incoming_headers,
        body,
        is_stream,
        upstream_cookie,
        auth_token,
        account,
        true,
    ) {
        Ok(resp) => resp,
        Err(err) => {
            log::warn!(
                "gateway stateless retry error: account_id={}, err={}",
                account.id,
                err
            );
            return StatelessRetryResult::NotTriggered;
        }
    };

    if let Some(alt_url) = alt_url {
        if matches!(response.status().as_u16(), 400 | 404) {
            if !super::backoff::sleep_with_exponential_jitter(
                Duration::from_millis(80),
                Duration::from_millis(500),
                2,
                request_deadline,
            ) {
                return StatelessRetryResult::Terminal {
                    status_code: 504,
                    message: "upstream total timeout exceeded".to_string(),
                };
            }
            match send_upstream_request(
                client,
                method,
                alt_url,
                request_deadline,
                request,
                incoming_headers,
                body,
                is_stream,
                upstream_cookie,
                auth_token,
                account,
                true,
            ) {
                Ok(resp) => {
                    response = resp;
                }
                Err(err) => {
                    log::warn!(
                        "gateway stateless alt retry error: account_id={}, err={}",
                        account.id,
                        err
                    );
                }
            }
        }
    }

    StatelessRetryResult::Upstream(response)
}

#[cfg(test)]
mod tests {
    use super::should_trigger_stateless_retry;

    #[test]
    fn stateless_retry_disables_403_when_challenge_retry_is_disabled() {
        assert!(!should_trigger_stateless_retry(403, false, true));
        assert!(should_trigger_stateless_retry(401, false, true));
        assert!(should_trigger_stateless_retry(404, false, true));
    }

    #[test]
    fn stateless_retry_keeps_403_when_challenge_retry_is_enabled() {
        assert!(should_trigger_stateless_retry(403, false, false));
    }

    #[test]
    fn stateless_retry_respects_session_affinity_guard() {
        assert!(!should_trigger_stateless_retry(401, true, false));
        assert!(should_trigger_stateless_retry(403, true, false));
        assert!(!should_trigger_stateless_retry(403, true, true));
    }
}



