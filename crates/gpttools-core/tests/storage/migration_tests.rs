use super::Storage;

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
