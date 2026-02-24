CREATE TABLE IF NOT EXISTS login_sessions (
  login_id TEXT PRIMARY KEY,
  code_verifier TEXT NOT NULL,
  state TEXT NOT NULL,
  status TEXT NOT NULL,
  error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
