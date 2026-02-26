CREATE TABLE IF NOT EXISTS model_options_cache (
  scope TEXT PRIMARY KEY,
  items_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

