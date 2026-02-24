ALTER TABLE usage_snapshots ADD COLUMN secondary_used_percent REAL;
ALTER TABLE usage_snapshots ADD COLUMN secondary_window_minutes INTEGER;
ALTER TABLE usage_snapshots ADD COLUMN secondary_resets_at INTEGER;
