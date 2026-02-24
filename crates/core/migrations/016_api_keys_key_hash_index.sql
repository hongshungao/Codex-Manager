CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash
  ON api_keys(key_hash);
