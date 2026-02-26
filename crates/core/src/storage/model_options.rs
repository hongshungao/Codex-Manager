use rusqlite::params;

use super::{ModelOptionsCacheRecord, Storage};

impl Storage {
    pub fn upsert_model_options_cache(
        &self,
        scope: &str,
        items_json: &str,
        updated_at: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO model_options_cache (scope, items_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(scope) DO UPDATE SET
               items_json = excluded.items_json,
               updated_at = excluded.updated_at",
            params![scope, items_json, updated_at],
        )?;
        Ok(())
    }

    pub fn get_model_options_cache(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Option<ModelOptionsCacheRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope, items_json, updated_at
             FROM model_options_cache
             WHERE scope = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([scope])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(ModelOptionsCacheRecord {
                scope: row.get(0)?,
                items_json: row.get(1)?,
                updated_at: row.get(2)?,
            }));
        }
        Ok(None)
    }
}

