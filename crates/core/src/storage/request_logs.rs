use rusqlite::{Result, Row};

use super::{request_log_query, RequestLog, Storage};

impl Storage {
    pub fn insert_request_log(&self, log: &RequestLog) -> Result<()> {
        self.conn.execute(
            "INSERT INTO request_logs (key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                &log.key_id,
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
        Ok(())
    }

    pub fn list_request_logs(&self, query: Option<&str>, limit: i64) -> Result<Vec<RequestLog>> {
        let normalized_limit = if limit <= 0 { 200 } else { limit.min(1000) };
        let mut out = Vec::new();

        match request_log_query::parse_request_log_query(query) {
            request_log_query::RequestLogQuery::All => {
                let mut stmt = self.conn.prepare(
                    "SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
                     FROM request_logs
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?1",
                )?;
                let mut rows = stmt.query([normalized_limit])?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::FieldLike { column, pattern } => {
                let sql = format!(
                    "SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
                     FROM request_logs
                     WHERE IFNULL({column}, '') LIKE ?1
                     ORDER BY created_at DESC, id DESC
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
                    "SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
                     FROM request_logs
                     WHERE {column} = ?1
                     ORDER BY created_at DESC, id DESC
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
                    "SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
                     FROM request_logs
                     WHERE status_code = ?1
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                )?;
                let mut rows = stmt.query((status, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::StatusRange(start, end) => {
                let mut stmt = self.conn.prepare(
                    "SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
                     FROM request_logs
                     WHERE status_code >= ?1 AND status_code <= ?2
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?3",
                )?;
                let mut rows = stmt.query((start, end, normalized_limit))?;
                while let Some(row) = rows.next()? {
                    out.push(map_request_log_row(row)?);
                }
            }
            request_log_query::RequestLogQuery::GlobalLike(pattern) => {
                let mut stmt = self.conn.prepare(
                    "SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
                     FROM request_logs
                     WHERE request_path LIKE ?1
                        OR method LIKE ?1
                        OR IFNULL(model,'') LIKE ?1
                        OR IFNULL(reasoning_effort,'') LIKE ?1
                        OR IFNULL(error,'') LIKE ?1
                        OR IFNULL(key_id,'') LIKE ?1
                        OR IFNULL(upstream_url,'') LIKE ?1
                        OR IFNULL(CAST(status_code AS TEXT),'') LIKE ?1
                     ORDER BY created_at DESC, id DESC
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
        self.conn.execute("DELETE FROM request_logs", [])?;
        Ok(())
    }

    pub(super) fn ensure_request_logs_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key_id TEXT,
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
        Ok(())
    }

    pub(super) fn ensure_request_log_reasoning_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "reasoning_effort", "TEXT")?;
        Ok(())
    }
}

fn map_request_log_row(row: &Row<'_>) -> Result<RequestLog> {
    Ok(RequestLog {
        key_id: row.get(0)?,
        request_path: row.get(1)?,
        method: row.get(2)?,
        model: row.get(3)?,
        reasoning_effort: row.get(4)?,
        upstream_url: row.get(5)?,
        status_code: row.get(6)?,
        error: row.get(7)?,
        created_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::Storage;

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
             SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
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
             SELECT key_id, request_path, method, model, reasoning_effort, upstream_url, status_code, error, created_at
             FROM request_logs
             WHERE key_id = 'gk_1'
             ORDER BY created_at DESC, id DESC
             LIMIT 100",
        );
        assert!(details
            .iter()
            .any(|detail| detail.contains("idx_request_logs_key_id_created_at")));
    }
}
