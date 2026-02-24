CREATE TABLE IF NOT EXISTS api_key_secrets (
  key_id TEXT PRIMARY KEY,
  key_value TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_key_secrets_updated_at ON api_key_secrets(updated_at);
