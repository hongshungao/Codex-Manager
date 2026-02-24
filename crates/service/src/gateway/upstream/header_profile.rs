use rand::RngCore;
use sha2::{Digest, Sha256};
use tiny_http::Request;

use super::super::IncomingHeaderSnapshot;

pub(super) const CODEX_CLIENT_VERSION: &str = "0.101.0";
pub(super) const CODEX_USER_AGENT: &str =
    "codex_cli_rs/0.101.0 (Mac OS 26.0.1; arm64) Apple_Terminal/464";
const CODEX_OPENAI_BETA: &str = "responses=experimental";
const CODEX_ORIGINATOR: &str = "codex_cli_rs";

pub(crate) struct CodexUpstreamHeaderInput<'a> {
    pub(crate) auth_token: &'a str,
    pub(crate) account_id: Option<&'a str>,
    pub(crate) upstream_cookie: Option<&'a str>,
    pub(crate) incoming_session_id: Option<&'a str>,
    pub(crate) fallback_session_id: Option<&'a str>,
    pub(crate) incoming_turn_state: Option<&'a str>,
    pub(crate) incoming_conversation_id: Option<&'a str>,
    pub(crate) fallback_conversation_id: Option<&'a str>,
    pub(crate) strip_session_affinity: bool,
    pub(crate) is_stream: bool,
    pub(crate) has_body: bool,
}

#[allow(dead_code)]
pub(crate) fn find_incoming_header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

#[allow(dead_code)]
pub(crate) fn derive_sticky_session_id(request: &Request) -> Option<String> {
    let incoming_headers = IncomingHeaderSnapshot::from_request(request);
    derive_sticky_session_id_from_headers(&incoming_headers)
}

#[allow(dead_code)]
pub(crate) fn derive_sticky_conversation_id(request: &Request) -> Option<String> {
    let incoming_headers = IncomingHeaderSnapshot::from_request(request);
    derive_sticky_conversation_id_from_headers(&incoming_headers)
}

pub(crate) fn derive_sticky_session_id_from_headers(
    incoming_headers: &IncomingHeaderSnapshot,
) -> Option<String> {
    derive_sticky_id_from_material(incoming_headers.sticky_key_material(), "session")
}

pub(crate) fn derive_sticky_conversation_id_from_headers(
    incoming_headers: &IncomingHeaderSnapshot,
) -> Option<String> {
    derive_sticky_id_from_material(incoming_headers.sticky_key_material(), "conversation")
}

pub(crate) fn build_codex_upstream_headers(
    input: CodexUpstreamHeaderInput<'_>,
) -> Vec<(String, String)> {
    let mut headers = Vec::with_capacity(12);
    headers.push((
        "Authorization".to_string(),
        format!("Bearer {}", input.auth_token),
    ));
    if input.has_body {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
    }
    headers.push((
        "Accept".to_string(),
        if input.is_stream {
            "text/event-stream"
        } else {
            "application/json"
        }
        .to_string(),
    ));
    headers.push(("Connection".to_string(), "Keep-Alive".to_string()));
    headers.push(("Version".to_string(), CODEX_CLIENT_VERSION.to_string()));
    headers.push(("Openai-Beta".to_string(), CODEX_OPENAI_BETA.to_string()));
    headers.push(("User-Agent".to_string(), CODEX_USER_AGENT.to_string()));
    headers.push(("Originator".to_string(), CODEX_ORIGINATOR.to_string()));
    headers.push((
        "Session_id".to_string(),
        resolve_session_id(
            input.incoming_session_id,
            input.fallback_session_id,
            input.strip_session_affinity,
        ),
    ));

    if !input.strip_session_affinity {
        if let Some(turn_state) = input.incoming_turn_state {
            headers.push(("x-codex-turn-state".to_string(), turn_state.to_string()));
        }
        if let Some(conversation_id) = input
            .incoming_conversation_id
            .or(input.fallback_conversation_id)
        {
            headers.push(("Conversation_id".to_string(), conversation_id.to_string()));
        }
    }

    if let Some(account_id) = input.account_id {
        headers.push(("Chatgpt-Account-Id".to_string(), account_id.to_string()));
    }
    if let Some(cookie) = input.upstream_cookie.filter(|value| !value.trim().is_empty()) {
        headers.push(("Cookie".to_string(), cookie.to_string()));
    }
    headers
}

fn resolve_session_id(
    incoming: Option<&str>,
    fallback_session_id: Option<&str>,
    strip_session_affinity: bool,
) -> String {
    if strip_session_affinity {
        return random_session_id();
    }
    if let Some(value) = incoming {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(value) = fallback_session_id {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    random_session_id()
}

fn stable_session_id_from_material(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn derive_sticky_id_from_material(key_material: Option<&str>, salt: &str) -> Option<String> {
    let key_material = key_material?;
    Some(stable_session_id_from_material(&format!("{salt}:{key_material}")))
}

fn random_session_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    // 对齐 codex/CLIProxy 的 Session_id 形态：UUID v4（8-4-4-4-12）
    // 版本位: xxxx0100；变体位: 10xxxxxx
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}
