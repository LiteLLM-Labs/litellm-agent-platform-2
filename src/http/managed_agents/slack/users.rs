use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::errors::GatewayError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUserProfile {
    pub display_name: Option<String>,
    pub image_48: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUser {
    pub id: String,
    pub name: String,
    pub real_name: Option<String>,
    pub profile: Option<SlackUserProfile>,
    pub is_bot: bool,
}

#[derive(Debug, Deserialize)]
struct SlackResponseMetadata {
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackUsersListResponse {
    ok: bool,
    members: Option<Vec<SlackUser>>,
    error: Option<String>,
    response_metadata: Option<SlackResponseMetadata>,
}

pub async fn list_users(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
) -> Result<Vec<SlackUser>, GatewayError> {
    let url = format!("{}/users.list", api_base_url.trim_end_matches('/'));
    let mut all: Vec<SlackUser> = Vec::new();
    let mut cursor = String::new();
    loop {
        let mut q = vec![("limit", "200".to_owned())];
        if !cursor.is_empty() {
            q.push(("cursor", cursor.clone()));
        }
        let r: SlackUsersListResponse = client
            .get(&url)
            .bearer_auth(bot_token)
            .query(&q)
            .send()
            .await
            .map_err(GatewayError::Upstream)?
            .json()
            .await
            .map_err(GatewayError::Upstream)?;
        if !r.ok {
            return Err(GatewayError::SandboxError(format!(
                "slack users.list failed: {}",
                r.error.unwrap_or_else(|| "unknown_error".to_owned())
            )));
        }
        all.extend(
            r.members
                .unwrap_or_default()
                .into_iter()
                .filter(|u| !u.is_bot && u.id != "USLACKBOT"),
        );
        cursor = r
            .response_metadata
            .and_then(|m| m.next_cursor)
            .unwrap_or_default();
        if cursor.is_empty() {
            break;
        }
    }
    Ok(all)
}
