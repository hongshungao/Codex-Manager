use rusqlite::{Result, Row};

use super::{now_ts, ApiKey, Storage};

const API_KEY_SELECT_SQL: &str = "SELECT
    k.id,
    k.name,
    COALESCE(p.default_model, k.model_slug) AS model_slug,
    COALESCE(p.reasoning_effort, k.reasoning_effort) AS reasoning_effort,
    COALESCE(p.client_type, 'codex') AS client_type,
    COALESCE(p.protocol_type, 'openai_compat') AS protocol_type,
    COALESCE(p.auth_scheme, 'authorization_bearer') AS auth_scheme,
    p.upstream_base_url,
    p.static_headers_json,
    k.key_hash,
    k.status,
    k.created_at,
    k.last_used_at
 FROM api_keys k
 LEFT JOIN api_key_profiles p ON p.key_id = k.id";

impl Storage {
    pub fn insert_api_key(&self, key: &ApiKey) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO api_keys (id, name, model_slug, reasoning_effort, key_hash, status, created_at, last_used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                &key.id,
                &key.name,
                &key.model_slug,
                &key.reasoning_effort,
                &key.key_hash,
                &key.status,
                key.created_at,
                &key.last_used_at,
            ),
        )?;
        self.conn.execute(
            "INSERT INTO api_key_profiles (key_id, client_type, protocol_type, auth_scheme, upstream_base_url, static_headers_json, default_model, reasoning_effort, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(key_id) DO UPDATE SET
               client_type = excluded.client_type,
               protocol_type = excluded.protocol_type,
               auth_scheme = excluded.auth_scheme,
               upstream_base_url = excluded.upstream_base_url,
               static_headers_json = excluded.static_headers_json,
               default_model = excluded.default_model,
               reasoning_effort = excluded.reasoning_effort,
               updated_at = excluded.updated_at",
            (
                &key.id,
                &key.client_type,
                &key.protocol_type,
                &key.auth_scheme,
                &key.upstream_base_url,
                &key.static_headers_json,
                &key.model_slug,
                &key.reasoning_effort,
                key.created_at,
                now_ts(),
            ),
        )?;
        Ok(())
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKey>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{API_KEY_SELECT_SQL} ORDER BY k.created_at DESC"))?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_api_key_row(row)?);
        }
        Ok(out)
    }

    pub fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let mut stmt = self.conn.prepare(&format!(
            "{API_KEY_SELECT_SQL}
             WHERE k.key_hash = ?1
             LIMIT 1"
        ))?;
        let mut rows = stmt.query([key_hash])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_api_key_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn find_api_key_by_id(&self, key_id: &str) -> Result<Option<ApiKey>> {
        let mut stmt = self.conn.prepare(&format!(
            "{API_KEY_SELECT_SQL}
             WHERE k.id = ?1
             LIMIT 1"
        ))?;
        let mut rows = stmt.query([key_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_api_key_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_api_key_last_used(&self, key_hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET last_used_at = ?1 WHERE key_hash = ?2",
            (now_ts(), key_hash),
        )?;
        Ok(())
    }

    pub fn update_api_key_status(&self, key_id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET status = ?1 WHERE id = ?2",
            (status, key_id),
        )?;
        Ok(())
    }

    pub fn update_api_key_model_slug(&self, key_id: &str, model_slug: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET model_slug = ?1 WHERE id = ?2",
            (model_slug, key_id),
        )?;
        Ok(())
    }

    pub fn update_api_key_model_config(
        &self,
        key_id: &str,
        model_slug: Option<&str>,
        reasoning_effort: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET model_slug = ?1, reasoning_effort = ?2 WHERE id = ?3",
            (model_slug, reasoning_effort, key_id),
        )?;
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                created_at,
                updated_at
            )
            SELECT
                id,
                'codex',
                'openai_compat',
                'authorization_bearer',
                NULL,
                NULL,
                ?2,
                ?3,
                ?4,
                ?4
            FROM api_keys
            WHERE id = ?1
            ON CONFLICT(key_id) DO UPDATE SET
                default_model = excluded.default_model,
                reasoning_effort = excluded.reasoning_effort,
                updated_at = excluded.updated_at",
            (key_id, model_slug, reasoning_effort, now),
        )?;
        Ok(())
    }

    pub fn update_api_key_profile_config(
        &self,
        key_id: &str,
        client_type: &str,
        protocol_type: &str,
        auth_scheme: &str,
        upstream_base_url: Option<&str>,
        static_headers_json: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                created_at,
                updated_at
            )
            SELECT
                id,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                model_slug,
                reasoning_effort,
                created_at,
                ?7
            FROM api_keys
            WHERE id = ?1
            ON CONFLICT(key_id) DO UPDATE SET
                client_type = excluded.client_type,
                protocol_type = excluded.protocol_type,
                auth_scheme = excluded.auth_scheme,
                upstream_base_url = excluded.upstream_base_url,
                static_headers_json = excluded.static_headers_json,
                updated_at = excluded.updated_at",
            (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                now_ts(),
            ),
        )?;
        Ok(())
    }

    pub fn delete_api_key(&self, key_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM api_keys WHERE id = ?1", [key_id])?;
        Ok(())
    }

    pub(super) fn ensure_api_key_model_column(&self) -> Result<()> {
        self.ensure_column("api_keys", "model_slug", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_api_key_reasoning_column(&self) -> Result<()> {
        self.ensure_column("api_keys", "reasoning_effort", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_api_key_profiles_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS api_key_profiles (
                key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
                client_type TEXT NOT NULL CHECK (client_type IN ('codex', 'claude_code')),
                protocol_type TEXT NOT NULL CHECK (protocol_type IN ('openai_compat', 'anthropic_native')),
                auth_scheme TEXT NOT NULL CHECK (auth_scheme IN ('authorization_bearer', 'x_api_key')),
                upstream_base_url TEXT,
                static_headers_json TEXT,
                default_model TEXT,
                reasoning_effort TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_key_profiles_client_protocol ON api_key_profiles(client_type, protocol_type)",
            [],
        )?;
        self.backfill_api_key_profiles()
    }

    fn backfill_api_key_profiles(&self) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                created_at,
                updated_at
            )
            SELECT
                id,
                'codex',
                'openai_compat',
                'authorization_bearer',
                NULL,
                NULL,
                model_slug,
                reasoning_effort,
                created_at,
                created_at
            FROM api_keys",
            [],
        )?;
        Ok(())
    }
}

fn map_api_key_row(row: &Row<'_>) -> Result<ApiKey> {
    Ok(ApiKey {
        id: row.get(0)?,
        name: row.get(1)?,
        model_slug: row.get(2)?,
        reasoning_effort: row.get(3)?,
        client_type: row.get(4)?,
        protocol_type: row.get(5)?,
        auth_scheme: row.get(6)?,
        upstream_base_url: row.get(7)?,
        static_headers_json: row.get(8)?,
        key_hash: row.get(9)?,
        status: row.get(10)?,
        created_at: row.get(11)?,
        last_used_at: row.get(12)?,
    })
}
