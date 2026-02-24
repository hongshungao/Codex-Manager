use rusqlite::{Result, Row};

use super::{Storage, UsageSnapshotRecord};

impl Storage {
    pub fn insert_usage_snapshot(&self, snap: &UsageSnapshotRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO usage_snapshots (account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                &snap.account_id,
                snap.used_percent,
                snap.window_minutes,
                snap.resets_at,
                snap.secondary_used_percent,
                snap.secondary_window_minutes,
                snap.secondary_resets_at,
                &snap.credits_json,
                snap.captured_at,
            ),
        )?;
        Ok(())
    }

    pub fn prune_usage_snapshots_for_account(&self, account_id: &str, retain: usize) -> Result<usize> {
        if retain == 0 {
            return Ok(0);
        }
        self.conn.execute(
            "DELETE FROM usage_snapshots
             WHERE account_id = ?1
               AND id NOT IN (
                 SELECT id
                 FROM usage_snapshots
                 WHERE account_id = ?1
                 ORDER BY captured_at DESC, id DESC
                 LIMIT ?2
               )",
            (account_id, retain as i64),
        )
    }

    pub fn usage_snapshot_count_for_account(&self, account_id: &str) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(1) FROM usage_snapshots WHERE account_id = ?1",
            [account_id],
            |row| row.get(0),
        )
    }

    pub fn latest_usage_snapshot(&self) -> Result<Option<UsageSnapshotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at FROM usage_snapshots ORDER BY captured_at DESC, id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_usage_snapshot_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn latest_usage_snapshot_for_account(
        &self,
        account_id: &str,
    ) -> Result<Option<UsageSnapshotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at
             FROM usage_snapshots
             WHERE account_id = ?1
             ORDER BY captured_at DESC, id DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_usage_snapshot_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn latest_usage_snapshots_by_account(&self) -> Result<Vec<UsageSnapshotRecord>> {
        // 中文注释：窗口函数 + 复合索引可稳定处理“同 captured_at 并发写入”场景；
        // 不这样做会依赖复杂子查询拼接，后续维护和优化都更难。
        let mut stmt = self.conn.prepare(
            "WITH ranked AS (
                SELECT
                    id,
                    account_id,
                    used_percent,
                    window_minutes,
                    resets_at,
                    secondary_used_percent,
                    secondary_window_minutes,
                    secondary_resets_at,
                    credits_json,
                    captured_at,
                    ROW_NUMBER() OVER (
                        PARTITION BY account_id
                        ORDER BY captured_at DESC, id DESC
                    ) AS rn
                FROM usage_snapshots
            )
            SELECT
                account_id,
                used_percent,
                window_minutes,
                resets_at,
                secondary_used_percent,
                secondary_window_minutes,
                secondary_resets_at,
                credits_json,
                captured_at
            FROM ranked
            WHERE rn = 1
            ORDER BY captured_at DESC, id DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_usage_snapshot_row(row)?);
        }
        Ok(out)
    }

    pub(super) fn ensure_usage_secondary_columns(&self) -> Result<()> {
        self.ensure_column("usage_snapshots", "secondary_used_percent", "REAL")?;
        self.ensure_column("usage_snapshots", "secondary_window_minutes", "INTEGER")?;
        self.ensure_column("usage_snapshots", "secondary_resets_at", "INTEGER")?;
        Ok(())
    }
}

fn map_usage_snapshot_row(row: &Row<'_>) -> Result<UsageSnapshotRecord> {
    Ok(UsageSnapshotRecord {
        account_id: row.get(0)?,
        used_percent: row.get(1)?,
        window_minutes: row.get(2)?,
        resets_at: row.get(3)?,
        secondary_used_percent: row.get(4)?,
        secondary_window_minutes: row.get(5)?,
        secondary_resets_at: row.get(6)?,
        credits_json: row.get(7)?,
        captured_at: row.get(8)?,
    })
}
