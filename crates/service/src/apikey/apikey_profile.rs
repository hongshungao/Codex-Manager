pub(crate) const CLIENT_CODEX: &str = "codex";
pub(crate) const PROTOCOL_OPENAI_COMPAT: &str = "openai_compat";
pub(crate) const PROTOCOL_ANTHROPIC_NATIVE: &str = "anthropic_native";
pub(crate) const PROTOCOL_AZURE_OPENAI: &str = "azure_openai";
pub(crate) const AUTH_BEARER: &str = "authorization_bearer";
pub(crate) const AUTH_X_API_KEY: &str = "x_api_key";
pub(crate) const AUTH_API_KEY: &str = "api_key";

fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

pub(crate) fn normalize_protocol_type(value: Option<String>) -> Result<String, String> {
    match value {
        Some(raw) => match normalize_key(&raw).as_str() {
            "openai" | "openai_compat" => Ok(PROTOCOL_OPENAI_COMPAT.to_string()),
            "anthropic" | "anthropic_native" => Ok(PROTOCOL_ANTHROPIC_NATIVE.to_string()),
            "azure" | "azure_openai" => Ok(PROTOCOL_AZURE_OPENAI.to_string()),
            other => Err(format!("unsupported protocol type: {other}")),
        },
        None => Ok(PROTOCOL_OPENAI_COMPAT.to_string()),
    }
}

pub(crate) fn profile_from_protocol(protocol_type: &str) -> Result<(String, String, String), String> {
    let protocol = normalize_protocol_type(Some(protocol_type.to_string()))?;
    let auth_scheme = if protocol == PROTOCOL_ANTHROPIC_NATIVE {
        AUTH_X_API_KEY.to_string()
    } else if protocol == PROTOCOL_AZURE_OPENAI {
        AUTH_API_KEY.to_string()
    } else {
        AUTH_BEARER.to_string()
    };
    Ok((CLIENT_CODEX.to_string(), protocol, auth_scheme))
}

pub(crate) fn normalize_upstream_base_url(
    value: Option<String>,
) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed =
        reqwest::Url::parse(trimmed.as_str()).map_err(|_| "invalid upstreamBaseUrl".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("invalid upstreamBaseUrl scheme".to_string());
    }
    Ok(Some(trimmed))
}

pub(crate) fn normalize_static_headers_json(
    value: Option<String>,
) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|_| "invalid staticHeadersJson: must be a JSON object".to_string())?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "invalid staticHeadersJson: must be a JSON object".to_string())?;
    for (name, value) in obj {
        if name.trim().is_empty() {
            return Err("invalid staticHeadersJson: header name is empty".to_string());
        }
        if !value.is_string() {
            return Err(format!(
                "invalid staticHeadersJson: header {name} value must be string"
            ));
        }
    }
    Ok(Some(trimmed.to_string()))
}
