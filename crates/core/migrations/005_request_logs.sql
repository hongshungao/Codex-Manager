CREATE TABLE IF NOT EXISTS request_logs (
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
);

CREATE INDEX IF NOT EXISTS idx_request_logs_created_at
  ON request_logs(created_at DESC);
