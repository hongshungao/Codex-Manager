use super::Storage;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_db_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-{name}-{}-{nanos}.db", process::id()))
}

#[test]
fn init_tracks_schema_migrations_and_is_idempotent() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("first init");
    storage.init().expect("second init");

    let applied_001: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '001_init'",
            [],
            |row| row.get(0),
        )
        .expect("count 001 migration");
    assert_eq!(applied_001, 1);

    let applied_005: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '005_request_logs'",
            [],
            |row| row.get(0),
        )
        .expect("count 005 migration");
    assert_eq!(applied_005, 1);

    let applied_012: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '012_request_logs_search_indexes'",
            [],
            |row| row.get(0),
        )
        .expect("count 012 migration");
    assert_eq!(applied_012, 1);

    let applied_013: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '013_drop_accounts_note_tags'",
            [],
            |row| row.get(0),
        )
        .expect("count 013 migration");
    assert_eq!(applied_013, 1);
    let applied_014: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '014_drop_accounts_workspace_name'",
            [],
            |row| row.get(0),
        )
        .expect("count 014 migration");
    assert_eq!(applied_014, 1);
    let applied_015: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '015_api_key_profiles'",
            [],
            |row| row.get(0),
        )
        .expect("count 015 migration");
    assert_eq!(applied_015, 1);
    let applied_016: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '016_api_keys_key_hash_index'",
            [],
            |row| row.get(0),
        )
        .expect("count 016 migration");
    assert_eq!(applied_016, 1);
    let applied_017: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '017_usage_snapshots_captured_id_index'",
            [],
            |row| row.get(0),
        )
        .expect("count 017 migration");
    assert_eq!(applied_017, 1);
    let applied_018: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '018_accounts_sort_updated_at_index'",
            [],
            |row| row.get(0),
        )
        .expect("count 018 migration");
    assert_eq!(applied_018, 1);
    let applied_022: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '022_request_token_stats'",
            [],
            |row| row.get(0),
        )
        .expect("count 022 migration");
    assert_eq!(applied_022, 1);

    assert!(!storage.has_column("accounts", "note").expect("check accounts.note"));
    assert!(!storage.has_column("accounts", "tags").expect("check accounts.tags"));
    assert!(!storage.has_column("accounts", "workspace_name").expect("check accounts.workspace_name"));
}

#[test]
fn file_open_enables_wal_and_normal_synchronous() {
    let path = temp_db_path("sqlite-pragmas");
    let storage = Storage::open(&path).expect("open file storage");

    let journal_mode: String = storage
        .conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("read journal mode");
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

    let synchronous: i64 = storage
        .conn
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .expect("read synchronous mode");
    assert_eq!(synchronous, 1);

    drop(storage);
    let _ = fs::remove_file(path);
}

#[test]
fn account_meta_sql_migration_coexists_with_legacy_compat_marker() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage
        .conn
        .execute_batch(
            "CREATE TABLE accounts (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                issuer TEXT NOT NULL,
                chatgpt_account_id TEXT,
                workspace_id TEXT,
                workspace_name TEXT,
                note TEXT,
                tags TEXT,
                group_name TEXT,
                sort INTEGER DEFAULT 0,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE login_sessions (
                login_id TEXT PRIMARY KEY,
                code_verifier TEXT NOT NULL,
                state TEXT NOT NULL,
                status TEXT NOT NULL,
                error TEXT,
                note TEXT,
                tags TEXT,
                group_name TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .expect("create tables with account meta columns");
    storage
        .ensure_migrations_table()
        .expect("ensure migration tracker");
    storage
        .conn
        .execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES ('compat_account_meta_columns', 1)",
            [],
        )
        .expect("insert legacy compat marker");

    storage
        .apply_sql_or_compat_migration(
            "011_account_meta_columns",
            include_str!("../../migrations/011_account_meta_columns.sql"),
            |s| s.ensure_account_meta_columns(),
        )
        .expect("apply 011 migration with fallback");

    let applied_011: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '011_account_meta_columns'",
            [],
            |row| row.get(0),
        )
        .expect("count 011 migration");
    assert_eq!(applied_011, 1);

    let legacy_compat_marker: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = 'compat_account_meta_columns'",
            [],
            |row| row.get(0),
        )
        .expect("count compat marker");
    assert_eq!(legacy_compat_marker, 1);
}

#[test]
fn sql_migration_can_fallback_to_compat_when_schema_already_exists() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage
        .conn
        .execute_batch(
            "CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                name TEXT,
                model_slug TEXT,
                key_hash TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER
            )",
        )
        .expect("create api_keys with model_slug");
    storage
        .ensure_migrations_table()
        .expect("ensure migration tracker");

    storage
        .apply_sql_or_compat_migration(
            "004_api_key_model",
            include_str!("../../migrations/004_api_key_model.sql"),
            |s| s.ensure_api_key_model_column(),
        )
        .expect("apply 004 migration with fallback");

    let applied_004: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = '004_api_key_model'",
            [],
            |row| row.get(0),
        )
        .expect("count 004 migration");
    assert_eq!(applied_004, 1);
}

#[test]
fn api_key_profile_migration_backfills_existing_keys() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage
        .conn
        .execute_batch(
            "CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                name TEXT,
                model_slug TEXT,
                reasoning_effort TEXT,
                key_hash TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER
            );
            INSERT INTO api_keys (id, name, model_slug, reasoning_effort, key_hash, status, created_at, last_used_at)
            VALUES ('key-1', 'k1', 'gpt-5', 'low', 'hash-1', 'active', 100, NULL);",
        )
        .expect("prepare api_keys");
    storage
        .ensure_migrations_table()
        .expect("ensure migration tracker");

    storage
        .apply_sql_or_compat_migration(
            "015_api_key_profiles",
            include_str!("../../migrations/015_api_key_profiles.sql"),
            |s| s.ensure_api_key_profiles_table(),
        )
        .expect("apply 015 migration with fallback");

    let profile_row: (String, String, String, String, Option<String>, Option<String>) = storage
        .conn
        .query_row(
            "SELECT client_type, protocol_type, auth_scheme, default_model, reasoning_effort, upstream_base_url
             FROM api_key_profiles
             WHERE key_id = 'key-1'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("load backfilled profile");

    assert_eq!(profile_row.0, "codex");
    assert_eq!(profile_row.1, "openai_compat");
    assert_eq!(profile_row.2, "authorization_bearer");
    assert_eq!(profile_row.3, "gpt-5");
    assert_eq!(profile_row.4.as_deref(), Some("low"));
    assert_eq!(profile_row.5, None);
}

#[test]
fn key_hash_index_migration_adds_api_keys_index() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let index_sql: String = storage
        .conn
        .query_row(
            "SELECT sql
             FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_api_keys_key_hash'",
            [],
            |row| row.get(0),
        )
        .expect("load index definition");
    assert!(index_sql.contains("api_keys"));
    assert!(index_sql.contains("key_hash"));
}

#[test]
fn usage_snapshot_latest_index_migration_adds_captured_id_index() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let index_sql: String = storage
        .conn
        .query_row(
            "SELECT sql
             FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_usage_snapshots_captured_id'",
            [],
            |row| row.get(0),
        )
        .expect("load index definition");
    assert!(index_sql.contains("usage_snapshots"));
    assert!(index_sql.contains("captured_at DESC"));
    assert!(index_sql.contains("id DESC"));
}

#[test]
fn accounts_sort_index_migration_adds_sort_updated_at_index() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let index_sql: String = storage
        .conn
        .query_row(
            "SELECT sql
             FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_accounts_sort_updated_at'",
            [],
            |row| row.get(0),
        )
        .expect("load index definition");
    assert!(index_sql.contains("accounts"));
    assert!(index_sql.contains("sort ASC"));
    assert!(index_sql.contains("updated_at DESC"));
}

