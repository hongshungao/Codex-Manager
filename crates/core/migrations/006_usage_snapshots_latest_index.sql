CREATE INDEX IF NOT EXISTS idx_usage_snapshots_account_captured_id
  ON usage_snapshots(account_id, captured_at DESC, id DESC);
