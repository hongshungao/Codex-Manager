use rusqlite::{Connection, Result};
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

mod accounts;
mod api_keys;
mod events;
mod request_log_query;
mod request_logs;
mod request_token_stats;
mod tokens;
mod usage;
mod model_options;

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub issuer: String,
    pub chatgpt_account_id: Option<String>,
    pub workspace_id: Option<String>,
    pub group_name: Option<String>,
    pub sort: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub account_id: String,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub api_key_access_token: Option<String>,
    pub last_refresh: i64,
}

#[derive(Debug, Clone)]
pub struct LoginSession {
    pub login_id: String,
    pub code_verifier: String,
    pub state: String,
    pub status: String,
    pub error: Option<String>,
    pub note: Option<String>,
    pub tags: Option<String>,
    pub group_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UsageSnapshotRecord {
    pub account_id: String,
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub credits_json: Option<String>,
    pub captured_at: i64,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub account_id: Option<String>,
    pub event_type: String,
    pub message: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct RequestLog {
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub request_path: String,
    pub method: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub upstream_url: Option<String>,
    pub status_code: Option<i64>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct RequestTokenStat {
    pub request_log_id: i64,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct RequestLogTodaySummary {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct ApiKey {
    pub id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
    pub client_type: String,
    pub protocol_type: String,
    pub auth_scheme: String,
    pub upstream_base_url: Option<String>,
    pub static_headers_json: Option<String>,
    pub key_hash: String,
    pub status: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ModelOptionsCacheRecord {
    pub scope: String,
    pub items_json: String,
    pub updated_at: i64,
}

#[derive(Debug)]
pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        // 中文注释：并发写入时给 SQLite 一点等待时间，避免瞬时 lock 导致请求直接失败。
        conn.busy_timeout(Duration::from_millis(3000))?;
        // 中文注释：文件库启用 WAL + NORMAL，可明显降低并发读写互斥开销；
        // 仅在 open(path) 上设置，避免影响 open_in_memory 的行为预期。
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.busy_timeout(Duration::from_millis(3000))?;
        Ok(Self { conn })
    }

    pub fn init(&self) -> Result<()> {
        self.ensure_migrations_table()?;

        self.apply_sql_migration("001_init", include_str!("../../migrations/001_init.sql"))?;
        self.apply_sql_migration(
            "002_login_sessions",
            include_str!("../../migrations/002_login_sessions.sql"),
        )?;
        self.apply_sql_migration(
            "003_api_keys",
            include_str!("../../migrations/003_api_keys.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "004_api_key_model",
            include_str!("../../migrations/004_api_key_model.sql"),
            |s| s.ensure_api_key_model_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "005_request_logs",
            include_str!("../../migrations/005_request_logs.sql"),
            |s| s.ensure_request_logs_table(),
        )?;
        self.apply_sql_migration(
            "006_usage_snapshots_latest_index",
            include_str!("../../migrations/006_usage_snapshots_latest_index.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "007_usage_secondary_columns",
            include_str!("../../migrations/007_usage_secondary_columns.sql"),
            |s| s.ensure_usage_secondary_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "008_token_api_key_access_token",
            include_str!("../../migrations/008_token_api_key_access_token.sql"),
            |s| s.ensure_token_api_key_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "009_api_key_reasoning_effort",
            include_str!("../../migrations/009_api_key_reasoning_effort.sql"),
            |s| s.ensure_api_key_reasoning_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "010_request_log_reasoning_effort",
            include_str!("../../migrations/010_request_log_reasoning_effort.sql"),
            |s| s.ensure_request_log_reasoning_column(),
        )?;

        // 中文注释：先走 SQL 迁移，遇到历史库重复列冲突再回退 compat；不这样写会导致老库和新库长期两套机制并存。
        self.apply_sql_or_compat_migration(
            "011_account_meta_columns",
            include_str!("../../migrations/011_account_meta_columns.sql"),
            |s| s.ensure_account_meta_columns(),
        )?;
        self.apply_sql_migration(
            "012_request_logs_search_indexes",
            include_str!("../../migrations/012_request_logs_search_indexes.sql"),
        )?;
        self.apply_sql_migration(
            "013_drop_accounts_note_tags",
            include_str!("../../migrations/013_drop_accounts_note_tags.sql"),
        )?;
        self.apply_sql_migration(
            "014_drop_accounts_workspace_name",
            include_str!("../../migrations/014_drop_accounts_workspace_name.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "015_api_key_profiles",
            include_str!("../../migrations/015_api_key_profiles.sql"),
            |s| s.ensure_api_key_profiles_table(),
        )?;
        self.apply_sql_migration(
            "016_api_keys_key_hash_index",
            include_str!("../../migrations/016_api_keys_key_hash_index.sql"),
        )?;
        self.apply_sql_migration(
            "017_usage_snapshots_captured_id_index",
            include_str!("../../migrations/017_usage_snapshots_captured_id_index.sql"),
        )?;
        self.apply_sql_migration(
            "018_accounts_sort_updated_at_index",
            include_str!("../../migrations/018_accounts_sort_updated_at_index.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "019_api_key_secrets",
            include_str!("../../migrations/019_api_key_secrets.sql"),
            |s| s.ensure_api_key_secrets_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "020_request_logs_account_tokens_cost",
            include_str!("../../migrations/020_request_logs_account_tokens_cost.sql"),
            |s| s.ensure_request_log_account_tokens_cost_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "021_request_logs_cached_reasoning_tokens",
            include_str!("../../migrations/021_request_logs_cached_reasoning_tokens.sql"),
            |s| s.ensure_request_log_cached_reasoning_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "022_request_token_stats",
            include_str!("../../migrations/022_request_token_stats.sql"),
            |s| s.ensure_request_token_stats_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "023_request_token_stats_total_tokens",
            include_str!("../../migrations/023_request_token_stats_total_tokens.sql"),
            |s| s.ensure_request_token_stats_table(),
        )?;
        self.apply_sql_migration(
            "024_model_options_cache",
            include_str!("../../migrations/024_model_options_cache.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "025_tokens_refresh_schedule",
            include_str!("../../migrations/025_tokens_refresh_schedule.sql"),
            |s| s.ensure_token_refresh_schedule_columns(),
        )?;
        self.ensure_request_token_stats_table()?;
        Ok(())
    }

    pub fn insert_login_session(&self, session: &LoginSession) -> Result<()> {
        self.conn.execute(
            "INSERT INTO login_sessions (login_id, code_verifier, state, status, error, note, tags, group_name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                &session.login_id,
                &session.code_verifier,
                &session.state,
                &session.status,
                &session.error,
                &session.note,
                &session.tags,
                &session.group_name,
                session.created_at,
                session.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn get_login_session(&self, login_id: &str) -> Result<Option<LoginSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT login_id, code_verifier, state, status, error, note, tags, group_name, created_at, updated_at FROM login_sessions WHERE login_id = ?1",
        )?;
        let mut rows = stmt.query([login_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(LoginSession {
                login_id: row.get(0)?,
                code_verifier: row.get(1)?,
                state: row.get(2)?,
                status: row.get(3)?,
                error: row.get(4)?,
                note: row.get(5)?,
                tags: row.get(6)?,
                group_name: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_login_session_status(
        &self,
        login_id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE login_sessions SET status = ?1, error = ?2, updated_at = ?3 WHERE login_id = ?4",
            (status, error, now_ts(), login_id),
        )?;
        Ok(())
    }

    fn ensure_column(&self, table: &str, column: &str, column_type: &str) -> Result<()> {
        if self.has_column(table, column)? {
            return Ok(());
        }
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}");
        self.conn.execute(&sql, [])?;
        Ok(())
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let sql = format!("PRAGMA table_info({table})");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ensure_migrations_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    fn has_migration(&self, version: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM schema_migrations WHERE version = ?1 LIMIT 1")?;
        let mut rows = stmt.query([version])?;
        Ok(rows.next()?.is_some())
    }

    fn mark_migration(&self, version: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            (version, now_ts()),
        )?;
        Ok(())
    }

    fn apply_sql_migration(&self, version: &str, sql: &str) -> Result<()> {
        if self.has_migration(version)? {
            return Ok(());
        }
        self.conn.execute_batch(sql)?;
        self.mark_migration(version)
    }

    fn apply_sql_or_compat_migration<F>(&self, version: &str, sql: &str, compat: F) -> Result<()>
    where
        F: FnOnce(&Self) -> Result<()>,
    {
        if self.has_migration(version)? {
            return Ok(());
        }

        match self.conn.execute_batch(sql) {
            Ok(_) => {}
            Err(err) if Self::is_schema_conflict_error(&err) => {
                // 中文注释：历史库可能已通过旧版 ensure_* 加过列/表，不走 fallback 会让迁移在“重复列/表”上失败。
                compat(self)?;
            }
            Err(err) => return Err(err),
        }

        self.mark_migration(version)
    }

    fn is_schema_conflict_error(err: &rusqlite::Error) -> bool {
        match err {
            rusqlite::Error::SqliteFailure(_, maybe_message) => maybe_message
                .as_deref()
                .map(|message| {
                    message.contains("duplicate column name") || message.contains("already exists")
                })
                .unwrap_or(false),
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/storage/migration_tests.rs"]
mod migration_tests;

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
