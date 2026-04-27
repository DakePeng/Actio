//! Shared test helpers. Compiled only under `cfg(test)`.
//!
//! Centralizes the in-memory-SQLite-pool fixture that every `mod tests`
//! across `actio-core/src/` was independently re-defining. Closes
//! ISSUES.md #61.

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

use crate::repository::db::run_migrations;

/// Fresh in-memory SQLite pool with foreign keys enabled and all
/// migrations applied. Each call returns an isolated database; tests
/// that need a clean slate should call this in their setup.
pub async fn fresh_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}
