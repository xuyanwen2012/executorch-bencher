use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug)]
pub struct DbError(String);

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DbError {}

pub async fn connect_and_migrate(database_url: &str) -> Result<SqlitePool, DbError> {
    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(|err| DbError(format!("invalid database URL: {err}")))?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_millis(5000))
        .synchronous(SqliteSynchronous::Full)
        .journal_mode(SqliteJournalMode::Wal);

    let parent = options
        .get_filename()
        .parent()
        .filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)
            .map_err(|err| DbError(format!("failed to create database directory: {err}")))?;
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
        .map_err(|err| DbError(format!("failed to open database: {err}")))?;

    sqlx::migrate!()
        .run(&pool)
        .await
        .map_err(|err| DbError(format!("failed to apply database migrations: {err}")))?;

    Ok(pool)
}

pub async fn ping(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").fetch_one(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_pooled_connection_gets_the_configured_pragmas() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("benchmarks.sqlite3");
        let database_url = format!("sqlite://{}", db_path.display());

        let pool = connect_and_migrate(&database_url)
            .await
            .expect("failed to connect and migrate");

        // Acquire two connections concurrently to force the pool to open a
        // second physical connection, then verify both carry the pragmas -
        // not just the first one opened at startup.
        let mut conn1 = pool
            .acquire()
            .await
            .expect("failed to acquire connection 1");
        let mut conn2 = pool
            .acquire()
            .await
            .expect("failed to acquire connection 2");

        for conn in [&mut conn1, &mut conn2] {
            let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(&mut **conn)
                .await
                .expect("failed to read foreign_keys pragma");
            assert_eq!(foreign_keys, 1);

            let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
                .fetch_one(&mut **conn)
                .await
                .expect("failed to read busy_timeout pragma");
            assert_eq!(busy_timeout, 5000);
        }
    }

    #[tokio::test]
    async fn connect_and_migrate_fails_clearly_when_database_file_cannot_be_created() {
        // A path under a file (not a directory) can never be created as a directory,
        // so `create_dir_all` on its parent fails deterministically.
        let blocking_file = tempfile::NamedTempFile::new().expect("failed to create temp file");
        let bad_path = blocking_file
            .path()
            .join("nested")
            .join("benchmarks.sqlite3");
        let database_url = format!("sqlite://{}", bad_path.display());

        let err = connect_and_migrate(&database_url)
            .await
            .expect_err("connecting under a non-directory path should fail");
        assert!(
            err.to_string()
                .contains("failed to create database directory")
        );
    }
}
