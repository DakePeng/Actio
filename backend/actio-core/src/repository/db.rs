use anyhow::Context;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

pub async fn create_pool(database_url: &str) -> anyhow::Result<SqlitePool> {
    info!(database_url = %database_url, "Connecting to SQLite database");
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(database_url)
        .await
        .context("Failed to connect to SQLite database")?;

    // Set SQLite pragmas for desktop app usage
    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA busy_timeout = 5000;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(&pool)
        .await?;

    info!("SQLite database connected with WAL mode");
    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    info!("Running database migrations");
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Migration failed")?;
    info!("Migrations complete");
    Ok(())
}
