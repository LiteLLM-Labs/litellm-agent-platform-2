use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::errors::GatewayError;

pub async fn connect(database_url: &str) -> Result<PgPool, GatewayError> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .map_err(GatewayError::Database)
}

pub async fn migrate(pool: &PgPool) -> Result<(), GatewayError> {
    sqlx::migrate!("src/db/managed_agents/migrations")
        .run(pool)
        .await
        .map_err(GatewayError::Migration)
}

/// Like `migrate`, but drops `_sqlx_migrations` first so stale checksums never block CI.
/// Only use in test setup — never in production.
#[cfg(test)]
pub async fn migrate_fresh(pool: &PgPool) -> Result<(), GatewayError> {
    sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
        .execute(pool)
        .await
        .map_err(GatewayError::Database)?;
    migrate(pool).await
}
