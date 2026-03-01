use bytes::Bytes;
use codexmanager_core::storage::Account;
use std::time::Instant;
use tiny_http::Request;

fn extract_prompt_cache_key(body: &[u8]) -> Option<String> {
    if body.is_empty() || body.len() > 64 * 1024 {
        return None;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return None;
    };
    value
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn should_compact_upstream_headers() -> bool {
    super::super::cpa_no_cookie_header_mode_enabled()
}

pub(super) fn send_upstream_request(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    target_url: &str,
    request_deadline: Option<Instant>,
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let attempt_started_at = Instant::now();
    let compact_headers_mode = should_compact_upstream_headers();
    let prompt_cache_key = if strip_session_affinity {
        None
    } else {
        extract_prompt_cache_key(body.as_ref())
    };
    let mut incoming_session_id = incoming_headers.session_id();
    let mut incoming_conversation_id = incoming_headers.conversation_id();
    if compact_headers_mode && prompt_cache_key.is_some() {
        // 中文注释：在请求头收敛策略下，prompt_cache_key 命中时优先绑定新的会话锚点，
        // 避免透传旧会话 id 造成跨账号粘性。
        incoming_session_id = None;
        incoming_conversation_id = None;
    }
    let remote = request.remote_addr();
    let mut derived_session_id = if !strip_session_affinity && incoming_session_id.is_none() {
        super::header_profile::derive_sticky_session_id_from_headers_with_remote(
            incoming_headers,
            remote.copied(),
        )
    } else {
        None
    };
    let mut derived_conversation_id = if !strip_session_affinity && incoming_conversation_id.is_none() {
        super::header_profile::derive_sticky_conversation_id_from_headers_with_remote(
            incoming_headers,
            remote.copied(),
        )
    } else {
        None
    };

    // 中文注释：参考 CLIProxyAPI 的 claude 兼容逻辑：当 prompt_cache_key 存在时，
    // 需要将 Session_id/Conversation_id 与其对齐，否则更容易触发 upstream challenge。
    if !strip_session_affinity {
        if let Some(cache_key) = prompt_cache_key.as_ref() {
            derived_session_id = Some(cache_key.clone());
            derived_conversation_id = Some(cache_key.clone());
        }
    }
    let account_id = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref());
    let include_account_id =
        !compact_headers_mode && !super::super::is_openai_api_base(target_url);
    let header_input = super::header_profile::CodexUpstreamHeaderInput {
        auth_token,
        account_id,
        include_account_id,
        include_openai_beta: !compact_headers_mode,
        upstream_cookie,
        incoming_session_id,
        fallback_session_id: derived_session_id.as_deref(),
        incoming_turn_state: incoming_headers.turn_state(),
        include_turn_state: !compact_headers_mode,
        incoming_conversation_id,
        fallback_conversation_id: derived_conversation_id.as_deref(),
        include_conversation_id: !compact_headers_mode,
        strip_session_affinity,
        is_stream,
        has_body: !body.is_empty(),
    };
    let upstream_headers = super::header_profile::build_codex_upstream_headers(header_input);
    let build_request = |http: &reqwest::blocking::Client| {
        let mut builder = http.request(method.clone(), target_url);
        if let Some(timeout) = super::deadline::send_timeout(request_deadline, is_stream) {
            builder = builder.timeout(timeout);
        }
        for (name, value) in upstream_headers.iter() {
            builder = builder.header(name, value);
        }
        if !body.is_empty() {
            builder = builder.body(body.clone());
        }
        builder
    };

    let result = match build_request(client).send() {
        Ok(resp) => Ok(resp),
        Err(first_err) => {
            // 中文注释：进程启动后才开启系统代理时，旧单例 client 可能仍走旧网络路径；
            // 这里用 fresh client 立刻重试一次，避免必须手动重连服务。
            let fresh = super::super::fresh_upstream_client_for_account(account.id.as_str());
            match build_request(&fresh).send() {
                Ok(resp) => Ok(resp),
                Err(_) => Err(first_err),
            }
        }
    };
    let duration_ms = super::super::duration_to_millis(attempt_started_at.elapsed());
    super::super::metrics::record_gateway_upstream_attempt(duration_ms, result.is_err());
    result
}


