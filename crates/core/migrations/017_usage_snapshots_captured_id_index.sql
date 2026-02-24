CREATE INDEX IF NOT EXISTS idx_usage_snapshots_captured_id
  ON usage_snapshots(captured_at DESC, id DESC);
