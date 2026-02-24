use tiny_http::{Request, Response};

pub(crate) fn handle_gateway_request(mut request: Request) -> Result<(), String> {
    // 处理代理请求（鉴权后转发到上游）
    let debug = super::DEFAULT_GATEWAY_DEBUG;
    if request.method().as_str() == "OPTIONS" {
        let response = Response::empty(204);
        let _ = request.respond(response);
        return Ok(());
    }

    if request.url() == "/health" {
        let response = Response::from_string("ok");
        let _ = request.respond(response);
        return Ok(());
    }

    let _request_guard = super::begin_gateway_request();
    let trace_id = super::trace_log::next_trace_id();
    let request_path_for_log = super::normalize_models_path(request.url());
    let request_method_for_log = request.method().as_str().to_string();
    let validated =
        match super::local_validation::prepare_local_request(&mut request, trace_id.clone(), debug)
        {
        Ok(v) => v,
        Err(err) => {
            super::trace_log::log_request_start(
                trace_id.as_str(),
                "-",
                request_method_for_log.as_str(),
                request_path_for_log.as_str(),
                None,
                None,
                false,
                "-",
            );
            super::trace_log::log_request_final(
                trace_id.as_str(),
                err.status_code,
                None,
                None,
                Some(err.message.as_str()),
                0,
            );
            super::record_gateway_request_outcome(
                request_path_for_log.as_str(),
                err.status_code,
                None,
            );
            if let Some(storage) = super::open_storage() {
                super::write_request_log(
                    &storage,
                    None,
                    &request_path_for_log,
                    &request_method_for_log,
                    None,
                    None,
                    None,
                    Some(err.status_code),
                    Some(err.message.as_str()),
                );
            }
            let response = Response::from_string(err.message).with_status_code(err.status_code);
            let _ = request.respond(response);
            return Ok(());
        }
    };

    let trace_id_for_count_tokens = validated.trace_id.clone();
    let key_id_for_count_tokens = validated.key_id.clone();
    let protocol_type_for_count_tokens = validated.protocol_type.clone();
    let path_for_count_tokens = validated.path.clone();
    let request_method_for_count_tokens = validated.request_method.clone();
    let model_for_count_tokens = validated.model_for_log.clone();
    let reasoning_for_count_tokens = validated.reasoning_for_log.clone();
    let request = match super::maybe_respond_local_count_tokens(
        request,
        trace_id_for_count_tokens.as_str(),
        key_id_for_count_tokens.as_str(),
        protocol_type_for_count_tokens.as_str(),
        path_for_count_tokens.as_str(),
        request_method_for_count_tokens.as_str(),
        &validated.body,
        model_for_count_tokens.as_deref(),
        reasoning_for_count_tokens.as_deref(),
        &validated.storage,
    )? {
        Some(request) => request,
        None => return Ok(()),
    };

    super::proxy_validated_request(request, validated, debug)
}

