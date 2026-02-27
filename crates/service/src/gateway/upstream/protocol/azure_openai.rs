use bytes::Bytes;
use codexmanager_core::storage::Storage;
use reqwest::header::{HeaderName, HeaderValue};
use std::time::Instant;
use tiny_http::{Request, Response};

use crate::apikey_profile::PROTOCOL_AZURE_OPENAI;

fn parse_static_headers_json(raw: Option<&str>) -> Result<Vec<(HeaderName, HeaderValue)>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };
    let parsed: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "invalid staticHeadersJson".to_string())?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "invalid staticHeadersJson".to_string())?;

    let mut out = Vec::with_capacity(obj.len());
    for (name, value) in obj {
        let Some(value_text) = value.as_str() else {
            return Err(format!("invalid staticHeadersJson: header {name} value must be string"));
        };
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("invalid staticHeadersJson: header {name} is invalid"))?;
        let header_value = HeaderValue::from_str(value_text)
            .map_err(|_| format!("invalid staticHeadersJson: header {name} value is invalid"))?;
        out.push((header_name, header_value));
    }

    Ok(out)
}

fn has_api_key_header(headers: &[(HeaderName, HeaderValue)]) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.as_str().eq_ignore_ascii_case("api-key"))
}

fn respond_error(request: Request, status: u16, message: &str) {
    let response = Response::from_string(message.to_string()).with_status_code(status);
    let _ = request.respond(response);
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn proxy_azure_request(
    request: Request,
    storage: &Storage,
    trace_id: &str,
    key_id: &str,
    path: &str,
    request_method: &str,
    method: &reqwest::Method,
    body: &Bytes,
    is_stream: bool,
    response_adapter: super::super::super::ResponseAdapter,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    upstream_base_url: Option<&str>,
    static_headers_json: Option<&str>,
    request_deadline: Option<Instant>,
    started_at: Instant,
) -> Result<(), String> {
    let Some(base) = upstream_base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        let message = "azure endpoint missing: please configure upstream_base_url";
        super::super::super::record_gateway_request_outcome(path, 400, Some(PROTOCOL_AZURE_OPENAI));
        super::super::super::trace_log::log_request_final(
            trace_id,
            400,
            Some(key_id),
            None,
            Some(message),
            started_at.elapsed().as_millis(),
        );
        super::super::super::write_request_log(
            storage,
            Some(key_id),
            None,
            path,
            request_method,
            model_for_log,
            reasoning_for_log,
            None,
            Some(400),
            super::super::super::request_log::RequestLogUsage::default(),
            Some(message),
        );
        respond_error(request, 400, message);
        return Ok(());
    };

    let mut static_headers = match parse_static_headers_json(static_headers_json) {
        Ok(value) => value,
        Err(err) => {
            super::super::super::record_gateway_request_outcome(
                path,
                400,
                Some(PROTOCOL_AZURE_OPENAI),
            );
            super::super::super::trace_log::log_request_final(
                trace_id,
                400,
                Some(key_id),
                None,
                Some(err.as_str()),
                started_at.elapsed().as_millis(),
            );
            super::super::super::write_request_log(
                storage,
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                None,
                Some(400),
                super::super::super::request_log::RequestLogUsage::default(),
                Some(err.as_str()),
            );
            respond_error(request, 400, err.as_str());
            return Ok(());
        }
    };

    // 优先使用配置里显式填写的 api-key；仅在缺失时回退到旧逻辑（平台 Key 明文）。
    if !has_api_key_header(&static_headers) {
        let api_key = match storage.find_api_key_secret_by_id(key_id) {
            Ok(Some(value)) if !value.trim().is_empty() => value,
            Ok(_) => {
                let message = "azure api key missing: please set API Key in Azure fields";
                super::super::super::record_gateway_request_outcome(
                    path,
                    403,
                    Some(PROTOCOL_AZURE_OPENAI),
                );
                super::super::super::trace_log::log_request_final(
                    trace_id,
                    403,
                    Some(key_id),
                    None,
                    Some(message),
                    started_at.elapsed().as_millis(),
                );
                super::super::super::write_request_log(
                    storage,
                    Some(key_id),
                    None,
                    path,
                    request_method,
                    model_for_log,
                    reasoning_for_log,
                    None,
                    Some(403),
                    super::super::super::request_log::RequestLogUsage::default(),
                    Some(message),
                );
                respond_error(request, 403, message);
                return Ok(());
            }
            Err(err) => {
                let message = format!("storage read failed: {err}");
                super::super::super::record_gateway_request_outcome(
                    path,
                    500,
                    Some(PROTOCOL_AZURE_OPENAI),
                );
                super::super::super::trace_log::log_request_final(
                    trace_id,
                    500,
                    Some(key_id),
                    None,
                    Some(message.as_str()),
                    started_at.elapsed().as_millis(),
                );
                super::super::super::write_request_log(
                    storage,
                    Some(key_id),
                    None,
                    path,
                    request_method,
                    model_for_log,
                    reasoning_for_log,
                    None,
                    Some(500),
                    super::super::super::request_log::RequestLogUsage::default(),
                    Some(message.as_str()),
                );
                respond_error(request, 500, message.as_str());
                return Ok(());
            }
        };
        static_headers.push((
            HeaderName::from_static("api-key"),
            HeaderValue::from_str(api_key.trim())
                .map_err(|_| "invalid azure api key".to_string())?,
        ));
    }

    let (url, _) = super::super::super::compute_upstream_url(base, path);
    let client = super::super::super::upstream_client();
    let mut builder = client.request(method.clone(), &url);
    if let Some(timeout) = super::super::deadline::send_timeout(request_deadline, is_stream) {
        builder = builder.timeout(timeout);
    }

    let request_headers = static_headers.clone();
    for (name, value) in request_headers.iter() {
        builder = builder.header(name, value);
    }
    builder = builder.header(
        "Accept",
        if is_stream {
            "text/event-stream"
        } else {
            "application/json"
        },
    );
    if !body.is_empty() {
        builder = builder.header("Content-Type", "application/json");
        builder = builder.body(body.clone());
    }

    let attempt_started_at = Instant::now();
    let upstream = match builder.send() {
        Ok(resp) => {
            let duration_ms =
                super::super::super::duration_to_millis(attempt_started_at.elapsed());
            super::super::super::metrics::record_gateway_upstream_attempt(duration_ms, false);
            resp
        }
        Err(first_err) => {
            // 中文注释：系统代理在服务启动后才切换时，旧 client 可能沿用旧网络状态；
            // 这里用 fresh client 再试一次，避免必须重启/重连。
            let fresh_client = super::super::super::fresh_upstream_client();
            let mut retry_builder = fresh_client.request(method.clone(), &url);
            if let Some(timeout) = super::super::deadline::send_timeout(request_deadline, is_stream) {
                retry_builder = retry_builder.timeout(timeout);
            }
            for (name, value) in request_headers.iter() {
                retry_builder = retry_builder.header(name, value);
            }
            retry_builder = retry_builder.header(
                "Accept",
                if is_stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            );
            if !body.is_empty() {
                retry_builder = retry_builder.header("Content-Type", "application/json");
                retry_builder = retry_builder.body(body.clone());
            }
            match retry_builder.send() {
                Ok(resp) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(duration_ms, false);
                    resp
                }
                Err(second_err) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(duration_ms, true);
                    let message = format!(
                        "azure upstream error: {}; retry_after_fresh_client: {}",
                        first_err, second_err
                    );
                    super::super::super::record_gateway_request_outcome(
                        path,
                        502,
                        Some(PROTOCOL_AZURE_OPENAI),
                    );
                    super::super::super::trace_log::log_request_final(
                        trace_id,
                        502,
                        Some(key_id),
                        Some(url.as_str()),
                        Some(message.as_str()),
                        started_at.elapsed().as_millis(),
                    );
                    super::super::super::write_request_log(
                        storage,
                        Some(key_id),
                        None,
                        path,
                        request_method,
                        model_for_log,
                        reasoning_for_log,
                        Some(url.as_str()),
                        Some(502),
                        super::super::super::request_log::RequestLogUsage::default(),
                        Some(message.as_str()),
                    );
                    respond_error(request, 502, message.as_str());
                    return Ok(());
                }
            }
        }
    };

    let status_code = upstream.status().as_u16();
    let error_text = if status_code >= 400 {
        Some("azure upstream non-success")
    } else {
        None
    };
    let inflight_guard = super::super::super::acquire_account_inflight(key_id);
    let usage = super::super::super::respond_with_upstream(
        request,
        upstream,
        inflight_guard,
        response_adapter,
        is_stream,
    )?;

    super::super::super::record_gateway_request_outcome(path, status_code, Some(PROTOCOL_AZURE_OPENAI));
    super::super::super::trace_log::log_request_final(
        trace_id,
        status_code,
        Some(key_id),
        Some(url.as_str()),
        error_text,
        started_at.elapsed().as_millis(),
    );
    super::super::super::write_request_log(
        storage,
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        Some(url.as_str()),
        Some(status_code),
        super::super::super::request_log::RequestLogUsage {
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            reasoning_output_tokens: usage.reasoning_output_tokens,
        },
        error_text,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{has_api_key_header, parse_static_headers_json};

    #[test]
    fn static_headers_parse_ok_and_detect_api_key() {
        let headers = parse_static_headers_json(Some(
            r#"{"api-key":"k123","x-extra":"v","Content-Type":"application/json"}"#,
        ))
        .expect("parse headers");
        assert!(has_api_key_header(&headers));
    }

    #[test]
    fn static_headers_without_api_key_returns_false() {
        let headers = parse_static_headers_json(Some(r#"{"authorization":"Bearer x"}"#))
            .expect("parse headers");
        assert!(!has_api_key_header(&headers));
    }

    #[test]
    fn static_headers_invalid_value_rejected() {
        let err = parse_static_headers_json(Some(r#"{"api-key":123}"#))
            .expect_err("should reject non-string header value");
        assert!(err.contains("value must be string"));
    }
}
