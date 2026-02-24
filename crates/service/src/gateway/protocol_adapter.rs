use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;

mod prompt_cache;
mod request_mapping;
mod response_conversion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponseAdapter {
    Passthrough,
    AnthropicJson,
    AnthropicSse,
}

#[derive(Debug)]
pub(super) struct AdaptedGatewayRequest {
    pub(super) path: String,
    pub(super) body: Vec<u8>,
    pub(super) response_adapter: ResponseAdapter,
}

pub(super) fn adapt_request_for_protocol(
    protocol_type: &str,
    path: &str,
    body: Vec<u8>,
) -> Result<AdaptedGatewayRequest, String> {
    if protocol_type != PROTOCOL_ANTHROPIC_NATIVE {
        return Ok(AdaptedGatewayRequest {
            path: path.to_string(),
            body,
            response_adapter: ResponseAdapter::Passthrough,
        });
    }

    if path == "/v1/messages" || path.starts_with("/v1/messages?") {
        let (adapted_body, request_stream) =
            request_mapping::convert_anthropic_messages_request(&body)?;
        // 说明：non-stream 也统一走 /v1/responses。
        // 在部分账号/环境下 /v1/responses/compact 更容易触发 challenge 或非预期拦截。
        let adapted_path = "/v1/responses".to_string();
        return Ok(AdaptedGatewayRequest {
            path: adapted_path,
            body: adapted_body,
            response_adapter: if request_stream {
                ResponseAdapter::AnthropicSse
            } else {
                ResponseAdapter::AnthropicJson
            },
        });
    }

    Ok(AdaptedGatewayRequest {
        path: path.to_string(),
        body,
        response_adapter: ResponseAdapter::Passthrough,
    })
}

pub(super) fn adapt_upstream_response(
    adapter: ResponseAdapter,
    upstream_content_type: Option<&str>,
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    response_conversion::adapt_upstream_response(adapter, upstream_content_type, body)
}

pub(super) fn build_anthropic_error_body(message: &str) -> Vec<u8> {
    response_conversion::build_anthropic_error_body(message)
}
