use tiny_http::Request;

pub(super) fn read_request_body(request: &mut Request) -> Vec<u8> {
    // 中文注释：先把请求体读完再进入鉴权判断，避免客户端写流还在进行时被提前断开。
    let mut body = Vec::new();
    let _ = request.as_reader().read_to_end(&mut body);
    body
}

pub(super) fn extract_platform_key_or_error(
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    debug: bool,
) -> Result<String, super::LocalValidationError> {
    if let Some(platform_key) = incoming_headers.platform_key() {
        return Ok(platform_key.to_string());
    }

    if debug {
        let remote = request
            .remote_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "<none>".to_string());
        let auth_scheme = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .and_then(|h| h.value.as_str().split_whitespace().next())
            .unwrap_or("<none>");
        let header_names = request
            .headers()
            .iter()
            .map(|h| h.field.as_str().as_str())
            .collect::<Vec<_>>()
            .join(",");
        eprintln!(
            "gateway auth missing: url={}, remote={}, has_auth={}, auth_scheme={}, has_x_api_key={}, headers=[{}]",
            request.url(),
            remote,
            incoming_headers.has_authorization(),
            auth_scheme,
            incoming_headers.has_x_api_key(),
            header_names,
        );
    }

    Err(super::LocalValidationError::new(401, "missing api key"))
}
