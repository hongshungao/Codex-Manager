use codexmanager_core::auth::extract_token_exp;
use codexmanager_core::storage::{now_ts, Storage, Token};

use crate::auth_tokens::obtain_api_key;
use crate::usage_http::refresh_access_token;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TokenRefreshPersistResult {
    pub(crate) rotated_refresh_token: bool,
    pub(crate) rotated_id_token: bool,
}

pub(crate) fn refresh_and_persist_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> Result<TokenRefreshPersistResult, String> {
    let refreshed = refresh_access_token(issuer, client_id, &token.refresh_token)?;
    let rotated_refresh_token = refreshed
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_some();
    let rotated_id_token = refreshed
        .id_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_some();
    token.access_token = refreshed.access_token;

    if let Some(refresh_token) = refreshed.refresh_token {
        token.refresh_token = refresh_token;
    }

    if let Some(id_token) = refreshed.id_token {
        token.id_token = id_token.clone();
        if let Ok(api_key) = obtain_api_key(issuer, client_id, &id_token) {
            token.api_key_access_token = Some(api_key);
        }
    }

    token.last_refresh = now_ts();
    storage.insert_token(token).map_err(|err| err.to_string())?;
    let access_exp = extract_token_exp(&token.access_token);
    let next_refresh_at = access_exp.map(|exp| exp.saturating_sub(600));
    let _ = storage.update_token_refresh_schedule(&token.account_id, access_exp, next_refresh_at);
    log::info!(
        "event=token_refresh_persisted account_id={} rotated_refresh_token={} rotated_id_token={}",
        token.account_id,
        rotated_refresh_token,
        rotated_id_token
    );
    Ok(TokenRefreshPersistResult {
        rotated_refresh_token,
        rotated_id_token,
    })
}
