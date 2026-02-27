use codexmanager_core::usage::usage_endpoint;
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::sync::mpsc;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

static USAGE_HTTP_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
const USAGE_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const USAGE_HTTP_READ_TIMEOUT: Duration = Duration::from_secs(30);
const USAGE_HTTP_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);

fn read_json_with_timeout<T>(resp: reqwest::blocking::Response, read_timeout: Duration) -> Result<T, String>
where
    T: DeserializeOwned + Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(resp.json::<T>().map_err(|e| e.to_string()));
    });
    match rx.recv_timeout(read_timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "response read timed out after {}ms",
            read_timeout.as_millis()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("response read failed: worker disconnected".to_string())
        }
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct RefreshTokenResponse {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) id_token: Option<String>,
}

fn build_usage_http_client() -> Client {
    Client::builder()
        // 中文注释：轮询链路复用连接池可降低握手开销；不复用会在多账号刷新时放大短连接抖动。
        .connect_timeout(USAGE_HTTP_CONNECT_TIMEOUT)
        .timeout(USAGE_HTTP_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(8)
        .pool_idle_timeout(Some(Duration::from_secs(60)))
        .build()
        .unwrap_or_else(|_| Client::new())
}

pub(crate) fn usage_http_client() -> Client {
    let lock = USAGE_HTTP_CLIENT.get_or_init(|| RwLock::new(build_usage_http_client()));
    match lock.read() {
        Ok(client) => client.clone(),
        Err(_) => build_usage_http_client(),
    }
}

fn rebuild_usage_http_client() {
    let next = build_usage_http_client();
    let lock = USAGE_HTTP_CLIENT.get_or_init(|| RwLock::new(next.clone()));
    if let Ok(mut current) = lock.write() {
        *current = next;
    }
}

pub(crate) fn fetch_usage_snapshot(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    // 调用上游用量接口
    let url = usage_endpoint(base_url);
    let build_request = || {
        let client = usage_http_client();
        let mut req = client
            .get(&url)
            .header("Authorization", format!("Bearer {bearer}"));
        if let Some(workspace_id) = workspace_id {
            req = req.header("ChatGPT-Account-Id", workspace_id);
        }
        req
    };
    let resp = match build_request().send() {
        Ok(resp) => resp,
        Err(first_err) => {
            // 中文注释：代理在程序启动后才开启时，旧 client 可能沿用旧网络状态；这里自动重建并重试一次。
            rebuild_usage_http_client();
            let retried = build_request().send();
            match retried {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "{}; retry_after_client_rebuild: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    if !resp.status().is_success() {
        return Err(format!("usage endpoint status {}", resp.status()));
    }
    read_json_with_timeout(resp, USAGE_HTTP_READ_TIMEOUT)
}

pub(crate) fn refresh_access_token(
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<RefreshTokenResponse, String> {
    // 使用 refresh_token 获取新的 access_token
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}&scope=openid%20profile%20email",
        urlencoding::encode(refresh_token),
        urlencoding::encode(client_id)
    );
    let build_request = || {
        let client = usage_http_client();
        client
            .post(format!("{issuer}/oauth/token"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.clone())
    };
    let resp = match build_request().send() {
        Ok(resp) => resp,
        Err(first_err) => {
            rebuild_usage_http_client();
            let retried = build_request().send();
            match retried {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "{}; retry_after_client_rebuild: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    if !resp.status().is_success() {
        return Err(format!(
            "refresh token failed with status {}",
            resp.status()
        ));
    }
    read_json_with_timeout(resp, USAGE_HTTP_READ_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::usage_http_client;

    #[test]
    fn usage_http_client_is_cloneable() {
        let first = usage_http_client();
        let second = usage_http_client();
        let first_ptr = &first as *const reqwest::blocking::Client;
        let second_ptr = &second as *const reqwest::blocking::Client;
        assert_ne!(first_ptr, second_ptr);
    }
}
