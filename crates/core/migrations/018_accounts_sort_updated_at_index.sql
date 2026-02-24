CREATE INDEX IF NOT EXISTS idx_accounts_sort_updated_at
  ON accounts(sort ASC, updated_at DESC);
