use rusqlite::{Result, Row};

use super::{request_log_query, RequestLog, RequestLogTodaySummary, RequestTokenStat, Storage};

impl Storage {
    pub fn insert_request_log(&self, log: &RequestLog) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO request_logs (key_id, account_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                &log.key_id,
                &log.account_id,
                &log.request_path,
                &log.method,
                &log.model,
                &log.reasoning_effort,
                &log.upstream_url,
                log.status_code,
                &log.error,
                log.created_at,
            ),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn insert_request_log_with_token_stat(
        &self,
        log: &RequestLog,
        stat: &RequestTokenStat,
    ) -> Result<(i64, Option<String>)> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO request_logs (key_id, account_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                &log.key_id,
                &log.account_id,
                &log.request_path,
                &log.method,
                &log.model,
                &log.reasoning_effort,
                &log.upstream_url,
                log.status_code,
                &log.error,
                log.created_at,
            ),
        )?;
        let request_log_id = tx.last_insert_rowid();

        // 中文注释：token 统计写入失败不应阻塞 request log 保留（例如 sqlite busy/锁竞争）。
        // 这里保持“单事务单提交”，但 stat 失败时仍 commit request log。
        let token_stat_error = tx
            .execute(
                "INSERT INTO request_token_stats (
                    request_log_id, key_id, account_id, model,
                    input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                (
                    request_log_id,
                    &stat.key_id,
                    &stat.account_id,
                    &stat.model,
                    stat.input_tokens,
                    stat.cached_input_tokens,
                    stat.output_tokens,
                    stat.total_tokens,
                    stat.reasoning_output_tokens,
                    stat.estimated_cost_usd,
                    stat.created_at,
                ),
            )
            .err()
            .map(|err| err.to_string());

        tx.commit()?;
        Ok((request_log_id, token_stat_error))
    }

    pub fn list_request_logs(&self, query: Option<&str>, limit: i64) -> Result<Vec<RequestLog>> {
        let normalized_limit = if limit <= 0 { 200 } else { limit.min(1000) };
        let mut out = Vec::new();

        match request_log_query::parse_request_log_query(query) {
            request_log_query::RequestLogQuery::All => {
                let mut stmt = self.conn.prepare(
                    "SELECT
                        r.key_id, r.account_id, r.request_path, r.method, r.model, r.reasoning_effort, r.upstream_url, r.status_code,
                        t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                        r.error, r.created_at
                     FROM request_logs r
                     LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                     ORDER BY r.created_at DESC, r.id DESC
                     LIMIT ?1",
                )?;
                let mut rows = stmt.query([normalized_limit])?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::FieldLike { column, pattern } => {
                let sql = format!(
                    "SELECT
                        r.key_id, r.account_id, r.request_path, r.method, r.model, r.reasoning_effort, r.upstream_url, r.status_code,
                        t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                        r.error, r.created_at
                     FROM request_logs r
                     LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                     WHERE IFNULL(r.{column}, '') LIKE ?1
                     ORDER BY r.created_at DESC, r.id DESC
                     LIMIT ?2"
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let mut rows = stmt.query((pattern, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::FieldExact { column, value } => {
                let sql = format!(
                    "SELECT
                        r.key_id, r.account_id, r.request_path, r.method, r.model, r.reasoning_effort, r.upstream_url, r.status_code,
                        t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                        r.error, r.created_at
                     FROM request_logs r
                     LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                     WHERE r.{column} = ?1
                     ORDER BY r.created_at DESC, r.id DESC
                     LIMIT ?2"
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let mut rows = stmt.query((value, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::StatusExact(status) => {
                let mut stmt = self.conn.prepare(
                    "SELECT
                        r.key_id, r.account_id, r.request_path, r.method, r.model, r.reasoning_effort, r.upstream_url, r.status_code,
                        t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                        r.error, r.created_at
                     FROM request_logs r
                     LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                     WHERE r.status_code = ?1
                     ORDER BY r.created_at DESC, r.id DESC
                     LIMIT ?2",
                )?;
                let mut rows = stmt.query((status, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::StatusRange(start, end) => {
                let mut stmt = self.conn.prepare(
                    "SELECT
                        r.key_id, r.account_id, r.request_path, r.method, r.model, r.reasoning_effort, r.upstream_url, r.status_code,
                        t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                        r.error, r.created_at
                     FROM request_logs r
                     LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                     WHERE r.status_code >= ?1 AND r.status_code <= ?2
                     ORDER BY r.created_at DESC, r.id DESC
                     LIMIT ?3",
                )?;
                let mut rows = stmt.query((start, end, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::GlobalLike(pattern) => {
                let mut stmt = self.conn.prepare(
                    "SELECT
                        r.key_id, r.account_id, r.request_path, r.method, r.model, r.reasoning_effort, r.upstream_url, r.status_code,
                        t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                        r.error, r.created_at
                     FROM request_logs r
                     LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                     WHERE r.request_path LIKE ?1
                        OR r.method LIKE ?1
                        OR IFNULL(r.account_id,'') LIKE ?1
                        OR IFNULL(r.model,'') LIKE ?1
                        OR IFNULL(r.reasoning_effort,'') LIKE ?1
                        OR IFNULL(r.error,'') LIKE ?1
                        OR IFNULL(r.key_id,'') LIKE ?1
                        OR IFNULL(r.upstream_url,'') LIKE ?1
                        OR IFNULL(CAST(r.status_code AS TEXT),'') LIKE ?1
                        OR IFNULL(CAST(t.input_tokens AS TEXT),'') LIKE ?1
                        OR IFNULL(CAST(t.cached_input_tokens AS TEXT),'') LIKE ?1
                        OR IFNULL(CAST(t.output_tokens AS TEXT),'') LIKE ?1
                        OR IFNULL(CAST(t.total_tokens AS TEXT),'') LIKE ?1
                        OR IFNULL(CAST(t.reasoning_output_tokens AS TEXT),'') LIKE ?1
                        OR IFNULL(CAST(t.estimated_cost_usd AS TEXT),'') LIKE ?1
                     ORDER BY r.created_at DESC, r.id DESC
                     LIMIT ?2",
                )?;
                let mut rows = stmt.query((pattern, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
        }

        Ok(out)
    }

    pub fn clear_request_logs(&self) -> Result<()> {
        // 只清理请求明细日志，保留 token 统计用于仪表盘历史用量与费用汇总。
        self.conn.execute("DELETE FROM request_logs", [])?;
        Ok(())
    }

    pub fn summarize_request_logs_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<RequestLogTodaySummary> {
        self.summarize_request_token_stats_between(start_ts, end_ts)
    }

    pub(super) fn ensure_request_logs_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key_id TEXT,
                account_id TEXT,
                request_path TEXT NOT NULL,
                method TEXT NOT NULL,
                model TEXT,
                reasoning_effort TEXT,
                upstream_url TEXT,
                status_code INTEGER,
                error TEXT,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON request_logs(created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_account_id_created_at ON request_logs(account_id, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at_id ON request_logs(created_at DESC, id DESC)",
            [],
        )?;
        Ok(())
    }

    pub(super) fn ensure_request_log_reasoning_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "reasoning_effort", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_account_tokens_cost_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "account_id", "TEXT")?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_account_id_created_at ON request_logs(account_id, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at_id ON request_logs(created_at DESC, id DESC)",
            [],
        )?;
        Ok(())
    }

    pub(super) fn ensure_request_log_cached_reasoning_columns(&self) -> Result<()> {
        Ok(())
    }
}

fn map_request_log_row(row: &Row<'_>) -> Result<RequestLog> {
    Ok(RequestLog {
        key_id: row.get(0)?,
        account_id: row.get(1)?,
        request_path: row.get(2)?,
        method: row.get(3)?,
        model: row.get(4)?,
        reasoning_effort: row.get(5)?,
        upstream_url: row.get(6)?,
        status_code: row.get(7)?,
        input_tokens: row.get(8)?,
        cached_input_tokens: row.get(9)?,
        output_tokens: row.get(10)?,
        total_tokens: row.get(11)?,
        reasoning_output_tokens: row.get(12)?,
        estimated_cost_usd: row.get(13)?,
        error: row.get(14)?,
        created_at: row.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{RequestLog, RequestTokenStat, Storage};

    fn collect_query_plan_details(storage: &Storage, sql: &str) -> Vec<String> {
        let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
        let mut rows = stmt.query([]).expect("query explain");
        let mut details = Vec::new();
        while let Some(row) = rows.next().expect("next explain row") {
            let detail: String = row.get(3).expect("detail");
            details.push(detail.to_ascii_lowercase());
        }
        details
    }

    #[test]
    fn method_exact_query_matches_composite_index() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let details = collect_query_plan_details(
            &storage,
            "EXPLAIN QUERY PLAN
             SELECT key_id, account_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
             FROM request_logs
             WHERE method = 'POST'
             ORDER BY created_at DESC, id DESC
             LIMIT 100",
        );
        assert!(details
            .iter()
            .any(|detail| detail.contains("idx_request_logs_method_created_at")));
    }

    #[test]
    fn key_exact_query_matches_composite_index() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let details = collect_query_plan_details(
            &storage,
            "EXPLAIN QUERY PLAN
             SELECT key_id, account_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
             FROM request_logs
             WHERE key_id = 'gk_1'
             ORDER BY created_at DESC, id DESC
             LIMIT 100",
        );
        assert!(details
            .iter()
            .any(|detail| detail.contains("idx_request_logs_key_id_created_at")));
    }

    #[test]
    fn insert_request_log_with_token_stat_is_visible_via_join() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        let created_at = 123456_i64;
        let log = RequestLog {
            key_id: Some("gk_1".to_string()),
            account_id: Some("acc_1".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5".to_string()),
            reasoning_effort: Some("medium".to_string()),
            upstream_url: Some("https://example.test".to_string()),
            status_code: Some(200),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at,
        };

        let stat = RequestTokenStat {
            request_log_id: 0,
            key_id: log.key_id.clone(),
            account_id: log.account_id.clone(),
            model: log.model.clone(),
            input_tokens: Some(10),
            cached_input_tokens: Some(1),
            output_tokens: Some(2),
            total_tokens: Some(12),
            reasoning_output_tokens: Some(3),
            estimated_cost_usd: Some(0.123),
            created_at,
        };

        let (_request_log_id, token_err) = storage
            .insert_request_log_with_token_stat(&log, &stat)
            .expect("insert request log with token stat");
        assert!(token_err.is_none(), "token stat should insert");

        let logs = storage
            .list_request_logs(None, 10)
            .expect("list request logs");
        assert_eq!(logs.len(), 1);
        let row = &logs[0];
        assert_eq!(row.request_path, log.request_path);
        assert_eq!(row.input_tokens, Some(10));
        assert_eq!(row.cached_input_tokens, Some(1));
        assert_eq!(row.output_tokens, Some(2));
        assert_eq!(row.total_tokens, Some(12));
        assert_eq!(row.reasoning_output_tokens, Some(3));
        assert_eq!(row.estimated_cost_usd, Some(0.123));
    }

    #[test]
    fn token_stat_failure_still_commits_request_log() {
        let storage = Storage::open_in_memory().expect("open");
        // Only create request_logs table, so request_token_stats insert fails.
        storage.ensure_request_logs_table().expect("ensure logs table");

        let created_at = 42_i64;
        let log = RequestLog {
            key_id: Some("gk_1".to_string()),
            account_id: Some("acc_1".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5".to_string()),
            reasoning_effort: None,
            upstream_url: None,
            status_code: Some(200),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at,
        };

        let stat = RequestTokenStat {
            request_log_id: 0,
            key_id: log.key_id.clone(),
            account_id: log.account_id.clone(),
            model: log.model.clone(),
            input_tokens: Some(1),
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            created_at,
        };

        let (_request_log_id, token_err) = storage
            .insert_request_log_with_token_stat(&log, &stat)
            .expect("insert request log with token stat");
        assert!(token_err.is_some(), "token stat insert should fail");

        let count: i64 = storage
            .conn
            .query_row("SELECT COUNT(1) FROM request_logs", [], |row| row.get(0))
            .expect("count request_logs");
        assert_eq!(count, 1);
    }
}
