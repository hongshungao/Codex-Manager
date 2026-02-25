CREATE TABLE IF NOT EXISTS request_token_stats (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  request_log_id INTEGER NOT NULL,
  key_id TEXT,
  account_id TEXT,
  model TEXT,
  input_tokens INTEGER,
  cached_input_tokens INTEGER,
  output_tokens INTEGER,
  reasoning_output_tokens INTEGER,
  estimated_cost_usd REAL,
  created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_token_stats_request_log_id
  ON request_token_stats(request_log_id);

CREATE INDEX IF NOT EXISTS idx_request_token_stats_created_at
  ON request_token_stats(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_token_stats_account_id_created_at
  ON request_token_stats(account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_token_stats_key_id_created_at
  ON request_token_stats(key_id, created_at DESC);
