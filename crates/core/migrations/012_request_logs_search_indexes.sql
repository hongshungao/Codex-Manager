CREATE INDEX IF NOT EXISTS idx_request_logs_status_code_created_at
  ON request_logs(status_code, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_method_created_at
  ON request_logs(method, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_key_id_created_at
  ON request_logs(key_id, created_at DESC);
