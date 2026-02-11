use gpttools_core::usage::usage_endpoint;
use reqwest::blocking::Client;
use std::time::Duration;

static USAGE_HTTP_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();

#[derive(serde::Deserialize)]
pub(crate) struct RefreshTokenResponse {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) id_token: Option<String>,
}

pub(crate) fn usage_http_client() -> &'static Client {
    USAGE_HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            // 中文注释：轮询链路复用连接池可降低握手开销；不复用会在多账号刷新时放大短连接抖动。
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(8)
            .pool_idle_timeout(Some(Duration::from_secs(60)))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

pub(crate) fn fetch_usage_snapshot(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    // 调用上游用量接口
    let url = usage_endpoint(base_url);
    let client = usage_http_client();
    let mut req = client
        .get(&url)
        .header("Authorization", format!("Bearer {bearer}"));
    if let Some(workspace_id) = workspace_id {
        req = req.header("ChatGPT-Account-Id", workspace_id);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("usage endpoint status {}", resp.status()));
    }
    resp.json().map_err(|e| e.to_string())
}

pub(crate) fn refresh_access_token(
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<RefreshTokenResponse, String> {
    // 使用 refresh_token 获取新的 access_token
    let client = usage_http_client();
    let resp = client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}&scope=openid%20profile%20email",
            urlencoding::encode(refresh_token),
            urlencoding::encode(client_id)
        ))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "refresh token failed with status {}",
            resp.status()
        ));
    }
    resp.json().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::usage_http_client;

    #[test]
    fn usage_http_client_reuses_singleton_instance() {
        let first = usage_http_client() as *const reqwest::blocking::Client;
        let second = usage_http_client() as *const reqwest::blocking::Client;
        assert_eq!(first, second);
    }
}
