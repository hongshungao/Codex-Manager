use tiny_http::Request;

#[derive(Clone, Default)]
pub(crate) struct IncomingHeaderSnapshot {
    authorization_present: bool,
    x_api_key_present: bool,
    authorization_bearer_strict: Option<String>,
    authorization_bearer_case_insensitive: Option<String>,
    x_api_key: Option<String>,
    session_id: Option<String>,
    turn_state: Option<String>,
    conversation_id: Option<String>,
}

impl IncomingHeaderSnapshot {
    pub(crate) fn from_request(request: &Request) -> Self {
        let mut snapshot = IncomingHeaderSnapshot::default();
        for header in request.headers() {
            if header.field.equiv("Authorization") {
                snapshot.authorization_present = true;
                let value = header.value.as_str().trim();
                if snapshot.authorization_bearer_strict.is_none() {
                    snapshot.authorization_bearer_strict = strict_bearer_token(value);
                }
                if snapshot.authorization_bearer_case_insensitive.is_none() {
                    snapshot.authorization_bearer_case_insensitive =
                        case_insensitive_bearer_token(value);
                }
                continue;
            }
            if header.field.equiv("x-api-key") {
                snapshot.x_api_key_present = true;
                if snapshot.x_api_key.is_none() {
                    let value = header.value.as_str().trim();
                    if !value.is_empty() {
                        snapshot.x_api_key = Some(value.to_string());
                    }
                }
                continue;
            }
            if snapshot.session_id.is_none() && header.field.equiv("session_id") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.session_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.turn_state.is_none() && header.field.equiv("x-codex-turn-state") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.turn_state = Some(value.to_string());
                }
                continue;
            }
            if snapshot.conversation_id.is_none() && header.field.equiv("conversation_id") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.conversation_id = Some(value.to_string());
                }
            }
        }
        snapshot
    }

    pub(crate) fn platform_key(&self) -> Option<&str> {
        self.x_api_key
            .as_deref()
            .or(self.authorization_bearer_strict.as_deref())
    }

    pub(crate) fn sticky_key_material(&self) -> Option<&str> {
        self.x_api_key
            .as_deref()
            .or(self.authorization_bearer_case_insensitive.as_deref())
    }

    pub(crate) fn has_authorization(&self) -> bool {
        self.authorization_present
    }

    pub(crate) fn has_x_api_key(&self) -> bool {
        self.x_api_key_present
    }

    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub(crate) fn turn_state(&self) -> Option<&str> {
        self.turn_state.as_deref()
    }

    pub(crate) fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }
}

fn strict_bearer_token(value: &str) -> Option<String> {
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn case_insensitive_bearer_token(value: &str) -> Option<String> {
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_bearer_parsing_matches_auth_extraction_behavior() {
        assert_eq!(strict_bearer_token("Bearer abc"), Some("abc".to_string()));
        assert_eq!(strict_bearer_token("bearer abc"), None);
        assert_eq!(strict_bearer_token("Bearer   "), None);
    }

    #[test]
    fn case_insensitive_bearer_parsing_matches_sticky_derivation_behavior() {
        assert_eq!(
            case_insensitive_bearer_token("Bearer abc"),
            Some("abc".to_string())
        );
        assert_eq!(
            case_insensitive_bearer_token("bearer abc"),
            Some("abc".to_string())
        );
        assert_eq!(case_insensitive_bearer_token("basic abc"), None);
        assert_eq!(case_insensitive_bearer_token("bearer   "), None);
    }
}
