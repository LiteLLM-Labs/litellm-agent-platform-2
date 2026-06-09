use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    db::managed_agents::{inbox, registry},
    errors::GatewayError,
};

use super::required_str;

pub async fn request_human_approval(
    pool: &PgPool,
    agent_id: &str,
    session_id: Option<&str>,
    arguments: Value,
) -> Result<Value, GatewayError> {
    let title = required_str(&arguments, "title")?.to_owned();
    let agent = registry::repository::get(pool, agent_id)
        .await?
        .ok_or_else(|| GatewayError::UnknownAgent(agent_id.to_owned()))?;
    let item = inbox::repository::create_approval(
        pool,
        title,
        session_id
            .map(str::to_owned)
            .or_else(|| optional_str(&arguments, "session_id")),
        Some(agent.name),
        optional_str(&arguments, "body"),
        arguments.get("arguments").cloned(),
    )
    .await?;
    wait_for_decision(pool, item.id).await
}

async fn wait_for_decision(pool: &PgPool, item_id: String) -> Result<Value, GatewayError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        let Some(item) = inbox::repository::get(pool, &item_id).await? else {
            return Ok(json!({
                "approval_id": item_id,
                "status": "missing"
            }));
        };
        if item.status != "pending" {
            return Ok(json!({
                "approval_id": item.id,
                "status": item.status,
                "feedback": item.feedback,
                "arguments": parse_args(item.args_json)
            }));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(json!({
                "approval_id": item.id,
                "status": "pending",
                "message": "approval is still pending in the inbox"
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

fn optional_str(arguments: &Value, field: &str) -> Option<String> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_args(args_json: Option<String>) -> Value {
    args_json
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_else(|| json!({}))
}
