use gpttools_core::storage::{now_ts, Storage, Token};

use crate::auth_tokens::obtain_api_key;
use crate::usage_http::refresh_access_token;

pub(crate) fn refresh_and_persist_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> Result<(), String> {
    let refreshed = refresh_access_token(issuer, client_id, &token.refresh_token)?;
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
    Ok(())
}
