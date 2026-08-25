//! The single store (ADR-2): embedded SQLite via `sqlx`, one file, WAL.
//! Every query lives here behind a typed repository function and is
//! compile-checked; offline metadata is committed under `.sqlx/`
//! (`cargo sqlx prepare --workspace`). Repository functions that write will
//! also fire the commit broadcast once the event bus lands (ADR-3).

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::time::Duration;

/// Embedded migrations, applied at process start (ADR-9). sqlx's migrations
/// table is the schema-version stamp.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// Open (creating if absent) the database file and apply pending migrations.
/// WAL for concurrent readers, foreign keys enforced, busy_timeout for the
/// single-writer discipline (ADR-2 consequences).
pub async fn open(path: &Path) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().connect_with(opts).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// In-memory database for integration tests — same migrations, same queries.
pub async fn open_in_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(true);
    // A single connection: each in-memory connection is its own database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Latest applied migration version — surfaced on `/healthz`.
pub async fn schema_version(pool: &SqlitePool) -> anyhow::Result<i64> {
    let row = sqlx::query!("SELECT MAX(version) AS version FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(row.version.unwrap_or(0))
}

pub mod instance_meta {
    use sqlx::SqlitePool;

    pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query!(
            "INSERT INTO instance_meta (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            key,
            value
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>> {
        let row = sqlx::query!("SELECT value FROM instance_meta WHERE key = ?", key)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|r| r.value))
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn migrations_apply_and_meta_roundtrips() {
        let pool = super::open_in_memory().await.unwrap();
        assert!(super::schema_version(&pool).await.unwrap() >= 1);
        super::instance_meta::set(&pool, "instance_id", "test").await.unwrap();
        assert_eq!(
            super::instance_meta::get(&pool, "instance_id").await.unwrap().as_deref(),
            Some("test")
        );
        assert_eq!(super::instance_meta::get(&pool, "absent").await.unwrap(), None);
    }
}
