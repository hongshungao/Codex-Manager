ALTER TABLE tokens ADD COLUMN access_token_exp INTEGER;
ALTER TABLE tokens ADD COLUMN next_refresh_at INTEGER;
ALTER TABLE tokens ADD COLUMN last_refresh_attempt_at INTEGER;
CREATE INDEX IF NOT EXISTS idx_tokens_next_refresh_at ON tokens(next_refresh_at);
