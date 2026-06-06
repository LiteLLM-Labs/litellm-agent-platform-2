use sqlx::{PgPool, Postgres, Transaction};

use crate::errors::GatewayError;

pub(super) struct SlackPromptLock<'a> {
    tx: Option<Transaction<'a, Postgres>>,
}

impl<'a> SlackPromptLock<'a> {
    pub(super) async fn acquire(pool: &'a PgPool, session_id: &str) -> Result<Self, GatewayError> {
        let mut tx = pool.begin().await.map_err(GatewayError::Database)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2))")
            .bind("slack_prompt")
            .bind(session_id)
            .execute(tx.as_mut())
            .await
            .map_err(GatewayError::Database)?;
        Ok(Self { tx: Some(tx) })
    }

    pub(super) async fn release(&mut self) -> Result<(), GatewayError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(GatewayError::Database)?;
        }
        Ok(())
    }
}
