CREATE TABLE IF NOT EXISTS api_key_profiles (
  key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
  client_type TEXT NOT NULL CHECK (client_type IN ('codex', 'claude_code')),
  protocol_type TEXT NOT NULL CHECK (protocol_type IN ('openai_compat', 'anthropic_native')),
  auth_scheme TEXT NOT NULL CHECK (auth_scheme IN ('authorization_bearer', 'x_api_key')),
  upstream_base_url TEXT,
  static_headers_json TEXT,
  default_model TEXT,
  reasoning_effort TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_key_profiles_client_protocol
  ON api_key_profiles(client_type, protocol_type);

INSERT OR IGNORE INTO api_key_profiles (
  key_id,
  client_type,
  protocol_type,
  auth_scheme,
  upstream_base_url,
  static_headers_json,
  default_model,
  reasoning_effort,
  created_at,
  updated_at
)
SELECT
  id,
  'codex',
  'openai_compat',
  'authorization_bearer',
  NULL,
  NULL,
  model_slug,
  reasoning_effort,
  created_at,
  created_at
FROM api_keys;
