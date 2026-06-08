use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::errors::GatewayError;

const MIGRATION_LOCK_KEY: i64 = 7_420_250_601;

pub async fn connect(database_url: &str) -> Result<PgPool, GatewayError> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .map_err(GatewayError::Database)
}

pub async fn migrate(pool: &PgPool) -> Result<(), GatewayError> {
    let mut connection = pool.acquire().await.map_err(GatewayError::Database)?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *connection)
        .await
        .map_err(GatewayError::Database)?;
    sqlx::migrate!("src/db/managed_agents/migrations")
        .run(&mut *connection)
        .await
        .map_err(GatewayError::Migration)?;
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *connection)
        .await
        .map_err(GatewayError::Database)?;
    Ok(())
}
