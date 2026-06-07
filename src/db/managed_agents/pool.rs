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
/// Holds a Postgres advisory lock (key 1) across the entire drop+migrate so concurrent
/// test binaries cannot interleave their drops and inserts.
/// Only for test setup — never call in production code.
pub async fn migrate_fresh(pool: &PgPool) -> Result<(), GatewayError> {
    // Advisory lock key 1 is ours; SQLx uses a key derived from the DB name (different).
    // pg_advisory_lock is re-entrant per session so no deadlock risk.
    sqlx::query("SELECT pg_advisory_lock(1)")
        .execute(pool)
        .await
        .map_err(GatewayError::Database)?;
    let result = async {
        sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
            .execute(pool)
            .await
            .map_err(GatewayError::Database)?;
        migrate(pool).await
    }
    .await;
    // Always release the lock even on error.
    let _ = sqlx::query("SELECT pg_advisory_unlock(1)")
        .execute(pool)
        .await;
    result
}
