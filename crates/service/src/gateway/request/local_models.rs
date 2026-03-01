use codexmanager_core::rpc::types::ModelOption;
use codexmanager_core::storage::now_ts;
use serde_json::json;
use tiny_http::Response;

const MODEL_CACHE_SCOPE_DEFAULT: &str = "default";
const MODELS_OWNED_BY: &str = "codexmanager";

fn build_openai_models_list(items: &[ModelOption]) -> String {
    let created = now_ts();
    let data = items
        .iter()
        .map(|item| {
            json!({
                "id": item.slug.as_str(),
                "object": "model",
                "created": created,
                "owned_by": MODELS_OWNED_BY,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "object": "list",
        "data": data,
    })
    .to_string()
}

fn fallback_model_options(model_for_log: Option<&str>) -> Vec<ModelOption> {
    let Some(slug) = model_for_log.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    vec![ModelOption {
        slug: slug.to_string(),
        display_name: slug.to_string(),
    }]
}

pub(super) fn maybe_respond_local_models(
    request: tiny_http::Request,
    trace_id: &str,
    key_id: &str,
    protocol_type: &str,
    path: &str,
    request_method: &str,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    storage: &codexmanager_core::storage::Storage,
) -> Result<Option<tiny_http::Request>, String> {
    let is_models_list = request_method.eq_ignore_ascii_case("GET")
        && (path == "/v1/models" || path.starts_with("/v1/models?"));
    if !is_models_list {
        return Ok(Some(request));
    }

    let mut fallback_reason: Option<String> = None;
    let cached_items = match storage.get_model_options_cache(MODEL_CACHE_SCOPE_DEFAULT) {
        Ok(Some(record)) => {
            serde_json::from_str::<Vec<ModelOption>>(&record.items_json).unwrap_or_default()
        }
        Ok(None) => Vec::new(),
        Err(err) => {
            let message = format!("model options cache read failed: {err}");
            super::trace_log::log_attempt_result(trace_id, "-", None, 503, Some(message.as_str()));
            super::trace_log::log_request_final(trace_id, 503, None, None, Some(message.as_str()), 0);
            super::record_gateway_request_outcome(path, 503, Some(protocol_type));
            super::write_request_log(
                storage,
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                None,
                Some(503),
                super::request_log::RequestLogUsage::default(),
                Some(message.as_str()),
            );
            let response = Response::from_string(message).with_status_code(503);
            let _ = request.respond(response);
            return Ok(None);
        }
    };

    let items = if !cached_items.is_empty() {
        cached_items
    } else {
        match super::fetch_models_for_picker() {
            Ok(fetched) if !fetched.is_empty() => {
                if let Ok(items_json) = serde_json::to_string(&fetched) {
                    if let Err(err) = storage.upsert_model_options_cache(
                        MODEL_CACHE_SCOPE_DEFAULT,
                        items_json.as_str(),
                        now_ts(),
                    ) {
                        log::warn!(
                            "event=gateway_model_options_cache_upsert_failed scope={} err={}",
                            MODEL_CACHE_SCOPE_DEFAULT,
                            err
                        );
                    }
                }
                fetched
            }
            Ok(_) => {
                let message = "models refresh returned empty list".to_string();
                fallback_reason = Some(message);
                fallback_model_options(model_for_log)
            }
            Err(err) => {
                let message = format!("models refresh failed: {err}");
                fallback_reason = Some(message);
                fallback_model_options(model_for_log)
            }
        }
    };

    let output = build_openai_models_list(&items);
    super::trace_log::log_attempt_result(trace_id, "-", None, 200, None);
    super::trace_log::log_request_final(trace_id, 200, None, None, None, 0);
    super::record_gateway_request_outcome(path, 200, Some(protocol_type));
    super::write_request_log(
        storage,
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        None,
        Some(200),
        super::request_log::RequestLogUsage::default(),
        fallback_reason.as_deref(),
    );
    let response = Response::from_string(output)
        .with_status_code(200)
        .with_header(
            tiny_http::Header::from_bytes(
                b"content-type".as_slice(),
                b"application/json".as_slice(),
            )
            .map_err(|_| "build content-type header failed".to_string())?,
        );
    let _ = request.respond(response);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn build_openai_models_list_outputs_expected_shape() {
        let items = vec![
            ModelOption {
                slug: "gpt-5.3-codex".to_string(),
                display_name: "GPT-5.3 Codex".to_string(),
            },
            ModelOption {
                slug: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
            },
        ];
        let output = build_openai_models_list(&items);
        let value: Value = serde_json::from_str(&output).expect("valid json");
        assert_eq!(value.get("object").and_then(Value::as_str), Some("list"));
        let data = value.get("data").and_then(Value::as_array).expect("data array");
        assert_eq!(data.len(), 2);
        for item in data {
            assert_eq!(item.get("object").and_then(Value::as_str), Some("model"));
            assert!(item.get("id").and_then(Value::as_str).is_some());
            assert!(item.get("created").and_then(Value::as_i64).is_some());
            assert_eq!(
                item.get("owned_by").and_then(Value::as_str),
                Some(MODELS_OWNED_BY)
            );
        }
    }
}
