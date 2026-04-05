use anyhow::Context;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::info;

pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    info!(database_url = %redact_database_url(database_url), "Connecting to database");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(database_url)
        .await
        .context("Failed to connect to database")?;
    info!("Database connected");
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    info!("Running database migrations");
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Migration failed")?;
    info!("Migrations complete");
    Ok(())
}

fn redact_database_url(database_url: &str) -> String {
    match database_url.rfind('@') {
        Some(index) => {
            let suffix = &database_url[index + 1..];
            format!("postgres://***@{suffix}")
        }
        None => "***".into(),
    }
}
