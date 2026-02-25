ALTER TABLE request_logs ADD COLUMN account_id TEXT;

CREATE INDEX IF NOT EXISTS idx_request_logs_account_id_created_at
  ON request_logs(account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_created_at_id
  ON request_logs(created_at DESC, id DESC);
