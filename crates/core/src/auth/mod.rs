use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";
pub const DEFAULT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_ORIGINATOR: &str = "codex_cli_rs";

#[derive(Debug, Clone, Deserialize)]
pub struct IdTokenClaims {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    pub auth: Option<AuthClaims>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthClaims {
    #[serde(default)]
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub chatgpt_plan_type: Option<String>,
    #[serde(default)]
    pub chatgpt_user_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

pub fn generate_pkce() -> PkceCodes {
    let mut bytes = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut bytes);
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

pub fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn parse_id_token_claims(token: &str) -> Result<IdTokenClaims, String> {
    let mut parts = token.split('.');
    let _header = parts.next();
    let payload = parts.next().ok_or_else(|| "invalid token".to_string())?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| e.to_string())?;
    let json = std::str::from_utf8(&decoded).map_err(|e| e.to_string())?;
    serde_json::from_str(json).map_err(|e| e.to_string())
}

pub fn extract_chatgpt_account_id(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let json = std::str::from_utf8(&decoded).ok()?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if let Some(v) = value.get("chatgpt_account_id").and_then(|v| v.as_str()) {
        if !v.trim().is_empty() {
            return Some(v.to_string());
        }
    }
    value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .filter(|v| !v.trim().is_empty())
}

pub fn extract_workspace_id(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let json = std::str::from_utf8(&decoded).ok()?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let keys = ["workspace_id", "chatgpt_account_id", "organization_id", "org_id"];
    for key in keys {
        if let Some(v) = value.get(key).and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                return Some(v.to_string());
            }
        }
    }
    if let Some(auth) = value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.as_object())
    {
        for key in keys {
            if let Some(v) = auth.get(key).and_then(|v| v.as_str()) {
                if !v.trim().is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

pub fn extract_workspace_name(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let json = std::str::from_utf8(&decoded).ok()?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let keys = [
        "organization_name",
        "org_name",
        "workspace_name",
        "team_name",
        "organization",
    ];
    for key in keys {
        if let Some(v) = value.get(key).and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                return Some(v.to_string());
            }
        }
    }
    if let Some(auth) = value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.as_object())
    {
        for key in keys {
            if let Some(v) = auth.get(key).and_then(|v| v.as_str()) {
                if !v.trim().is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

pub fn build_authorize_url(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    originator: &str,
    workspace_id: Option<&str>,
) -> String {
    let mut query = vec![
        ("response_type", "code".to_string()),
        ("client_id", client_id.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        (
            "scope",
            "openid profile email offline_access".to_string(),
        ),
        ("code_challenge", code_challenge.to_string()),
        ("code_challenge_method", "S256".to_string()),
        ("id_token_add_organizations", "true".to_string()),
        ("codex_cli_simplified_flow", "true".to_string()),
        ("state", state.to_string()),
        ("originator", originator.to_string()),
    ];
    if let Some(workspace_id) = workspace_id {
        query.push(("allowed_workspace_id", workspace_id.to_string()));
    }
    let qs = query
        .into_iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(&v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{issuer}/oauth/authorize?{qs}")
}

pub fn token_exchange_body_authorization_code(
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    code_verifier: &str,
) -> String {
    format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(client_id),
        urlencoding::encode(code_verifier)
    )
}

pub fn token_exchange_body_token_exchange(id_token: &str, client_id: &str) -> String {
    format!(
        "grant_type={}&client_id={}&requested_token={}&subject_token={}&subject_token_type={}",
        urlencoding::encode("urn:ietf:params:oauth:grant-type:token-exchange"),
        urlencoding::encode(client_id),
        urlencoding::encode("openai-api-key"),
        urlencoding::encode(id_token),
        urlencoding::encode("urn:ietf:params:oauth:token-type:id_token")
    )
}

pub fn device_usercode_url(issuer: &str) -> String {
    format!("{}/api/accounts/deviceauth/usercode", issuer.trim_end_matches('/'))
}

pub fn device_token_url(issuer: &str) -> String {
    format!("{}/api/accounts/deviceauth/token", issuer.trim_end_matches('/'))
}

pub fn device_verification_url(issuer: &str) -> String {
    format!("{}/codex/device", issuer.trim_end_matches('/'))
}

pub fn device_redirect_uri(issuer: &str) -> String {
    format!("{}/deviceauth/callback", issuer.trim_end_matches('/'))
}
