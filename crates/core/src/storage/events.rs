use rusqlite::Result;

use super::{Event, Storage};

impl Storage {
    pub fn insert_event(&self, event: &Event) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (account_id, type, message, created_at) VALUES (?1, ?2, ?3, ?4)",
            (
                &event.account_id,
                &event.event_type,
                &event.message,
                event.created_at,
            ),
        )?;
        Ok(())
    }

    pub fn event_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(1) FROM events", [], |row| row.get(0))
    }
}
