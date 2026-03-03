use super::*;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn reload_from_env_updates_timeout_and_cookie() {
    let _timeout_guard = EnvGuard::set(ENV_UPSTREAM_TOTAL_TIMEOUT_MS, "777");
    let _stream_timeout_guard = EnvGuard::set(ENV_UPSTREAM_STREAM_TIMEOUT_MS, "888");
    let _cookie_guard = EnvGuard::set(ENV_UPSTREAM_COOKIE, "cookie=abc");
    let _cpa_mode_guard = EnvGuard::set(ENV_CPA_NO_COOKIE_HEADER_MODE, "1");
    let _strict_allowlist_guard = EnvGuard::set(ENV_STRICT_REQUEST_PARAM_ALLOWLIST, "0");
    let _client_id_guard = EnvGuard::set(ENV_TOKEN_EXCHANGE_CLIENT_ID, "client-id-123");
    let _issuer_guard = EnvGuard::set(ENV_TOKEN_EXCHANGE_ISSUER, "https://issuer.example");

    reload_from_env();

    assert_eq!(upstream_total_timeout(), Some(Duration::from_millis(777)));
    assert_eq!(upstream_stream_timeout(), Some(Duration::from_millis(888)));
    assert_eq!(upstream_cookie().as_deref(), Some("cookie=abc"));
    assert!(cpa_no_cookie_header_mode_enabled());
    assert!(!strict_request_param_allowlist_enabled());
    assert_eq!(token_exchange_client_id(), "client-id-123");
    assert_eq!(
        token_exchange_default_issuer(),
        "https://issuer.example".to_string()
    );
}

#[test]
fn parse_proxy_list_env_limits_to_five_entries() {
    let _guard = EnvGuard::set(
    ENV_PROXY_LIST,
    "http://p1:8080,http://p2:8080;http://p3:8080\nhttp://p4:8080\rhttp://p5:8080,http://p6:8080",
);
    let parsed = parse_proxy_list_env();
    assert_eq!(parsed.len(), MAX_UPSTREAM_PROXY_POOL_SIZE);
    assert_eq!(parsed.first().map(String::as_str), Some("http://p1:8080"));
    assert_eq!(parsed.last().map(String::as_str), Some("http://p5:8080"));
}

#[test]
fn stable_proxy_index_is_deterministic() {
    let idx1 = stable_proxy_index("account-42", 5);
    let idx2 = stable_proxy_index("account-42", 5);
    assert_eq!(idx1, idx2);
    assert!(idx1.expect("index") < 5);
}
