use sqlx::PgPool;

pub async fn reset_tables(pool: &PgPool) {
    sqlx::query(
        r#"
        TRUNCATE
          "LiteLLM_ManagedAgentChannelsTable",
          "LiteLLM_ManagedAgentSlackOAuthStatesTable",
          "LiteLLM_ManagedAgentSlackEventsTable",
          "LiteLLM_CredentialsTable",
          "LiteLLM_ManagedAgentSlackThreadSessionsTable",
          "LiteLLM_ManagedAgentInboxItemsTable",
          "LiteLLM_ManagedAgentRunsTable",
          "LiteLLM_ManagedAgentFilesTable",
          "LiteLLM_ManagedAgentMemoriesTable",
          "LiteLLM_ManagedAgentsTable",
          "LiteLLM_ManagedAgentSessionsTable",
          "LiteLLM_ManagedAgentSkillsTable",
          "LiteLLM_SavedAgentsTable"
        CASCADE
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}
