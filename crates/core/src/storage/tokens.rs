use rusqlite::{Result, Row};

use super::{Storage, Token};

impl Storage {
    pub fn insert_token(&self, token: &Token) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO tokens (account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                &token.account_id,
                &token.id_token,
                &token.access_token,
                &token.refresh_token,
                &token.api_key_access_token,
                token.last_refresh,
            ),
        )?;
        Ok(())
    }

    pub fn token_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(1) FROM tokens", [], |row| row.get(0))
    }

    pub fn list_tokens(&self) -> Result<Vec<Token>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh FROM tokens",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_token_row(row)?);
        }
        Ok(out)
    }

    pub fn find_token_by_account_id(&self, account_id: &str) -> Result<Option<Token>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh
             FROM tokens
             WHERE account_id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_token_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub(super) fn ensure_token_api_key_column(&self) -> Result<()> {
        if self.has_column("tokens", "api_key_access_token")? {
            return Ok(());
        }
        self.conn.execute(
            "ALTER TABLE tokens ADD COLUMN api_key_access_token TEXT",
            [],
        )?;
        Ok(())
    }
}

fn map_token_row(row: &Row<'_>) -> Result<Token> {
    Ok(Token {
        account_id: row.get(0)?,
        id_token: row.get(1)?,
        access_token: row.get(2)?,
        refresh_token: row.get(3)?,
        api_key_access_token: row.get(4)?,
        last_refresh: row.get(5)?,
    })
}
