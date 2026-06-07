use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ManagedMcpServerRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub auth_type: String,
    #[serde(skip_serializing)]
    pub auth_value: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateManagedMcpServer {
    pub name: String,
    pub url: String,
    pub auth_type: Option<String>,
    pub auth_value: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateManagedMcpServer {
    pub name: Option<String>,
    pub url: Option<String>,
    pub auth_type: Option<String>,
    pub auth_value: Option<String>,
    pub description: Option<String>,
}
