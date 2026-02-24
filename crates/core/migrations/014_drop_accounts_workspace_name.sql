PRAGMA foreign_keys = OFF;

BEGIN TRANSACTION;

CREATE TABLE accounts_new (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  issuer TEXT NOT NULL,
  chatgpt_account_id TEXT,
  workspace_id TEXT,
  group_name TEXT,
  sort INTEGER DEFAULT 0,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

INSERT INTO accounts_new (
  id,
  label,
  issuer,
  chatgpt_account_id,
  workspace_id,
  group_name,
  sort,
  status,
  created_at,
  updated_at
)
SELECT
  id,
  label,
  issuer,
  chatgpt_account_id,
  workspace_id,
  group_name,
  sort,
  status,
  created_at,
  updated_at
FROM accounts;

DROP TABLE accounts;
ALTER TABLE accounts_new RENAME TO accounts;

COMMIT;

PRAGMA foreign_keys = ON;

