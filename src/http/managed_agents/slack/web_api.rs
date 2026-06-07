use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::errors::GatewayError;

#[derive(Debug, Deserialize)]
pub struct SlackAuthedUser {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct SlackOAuthAccessResponse {
    pub ok: bool,
    pub access_token: Option<String>,
    pub bot_user_id: Option<String>,
    pub team: Option<SlackTeam>,
    pub authed_user: Option<SlackAuthedUser>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SlackTeam {
    pub id: Option<String>,
    pub name: Option<String>,
}

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
struct SlackMessageResponse {
    ok: bool,
    ts: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackOkResponse {
    ok: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackUsersListResponse {
    ok: bool,
    members: Option<Vec<SlackUser>>,
    error: Option<String>,
    response_metadata: Option<SlackResponseMetadata>,
}

#[derive(Debug, Deserialize)]
struct SlackResponseMetadata {
    next_cursor: Option<String>,
}

pub async fn post_message_as(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
    channel: &str,
    thread_ts: &str,
    text: &str,
    username: Option<&str>,
) -> Result<String, GatewayError> {
    let response = post_message_raw(
        client,
        api_base_url,
        bot_token,
        channel,
        thread_ts,
        text,
        username,
    )
    .await?;
    if response.ok {
        return response.ts.ok_or_else(|| {
            GatewayError::SandboxError("slack chat.postMessage omitted ts".to_owned())
        });
    }
    if username.is_some() && response.error.as_deref() == Some("missing_scope") {
        let fallback = post_message_raw(
            client,
            api_base_url,
            bot_token,
            channel,
            thread_ts,
            text,
            None,
        )
        .await?;
        return match fallback.ok {
            true => fallback.ts.ok_or_else(|| {
                GatewayError::SandboxError("slack chat.postMessage omitted ts".to_owned())
            }),
            false => Err(slack_api_error("chat.postMessage", fallback.error)),
        };
    }
    Err(slack_api_error("chat.postMessage", response.error))
}

async fn post_message_raw(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
    channel: &str,
    thread_ts: &str,
    text: &str,
    username: Option<&str>,
) -> Result<SlackMessageResponse, GatewayError> {
    let mut body = json!({
        "channel": channel,
        "thread_ts": thread_ts,
        "text": truncate(text),
    });
    if let Some(username) = username.map(str::trim).filter(|value| !value.is_empty()) {
        body["username"] = username.into();
    }
    let response: SlackMessageResponse = client
        .post(method_url(api_base_url, "chat.postMessage"))
        .bearer_auth(bot_token)
        .json(&body)
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    Ok(response)
}

pub async fn update_message(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
    channel: &str,
    ts: &str,
    text: &str,
) -> Result<(), GatewayError> {
    let response: SlackOkResponse = client
        .post(method_url(api_base_url, "chat.update"))
        .bearer_auth(bot_token)
        .json(&json!({
            "channel": channel,
            "ts": ts,
            "text": truncate(text),
        }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    if response.ok {
        Ok(())
    } else {
        Err(slack_api_error("chat.update", response.error))
    }
}

pub async fn add_reaction(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
    channel: &str,
    timestamp: &str,
    name: &str,
) -> Result<(), GatewayError> {
    let response: SlackOkResponse = client
        .post(method_url(api_base_url, "reactions.add"))
        .bearer_auth(bot_token)
        .json(&json!({
            "channel": channel,
            "timestamp": timestamp,
            "name": name,
        }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    if response.ok {
        Ok(())
    } else {
        Err(slack_api_error("reactions.add", response.error))
    }
}

pub async fn oauth_access(
    client: &Client,
    api_base_url: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<SlackOAuthAccessResponse, GatewayError> {
    client
        .post(method_url(api_base_url, "oauth.v2.access"))
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)
}

pub async fn post_ephemeral(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
    channel: &str,
    user: &str,
    text: &str,
) -> Result<(), GatewayError> {
    let response: SlackOkResponse = client
        .post(method_url(api_base_url, "chat.postEphemeral"))
        .bearer_auth(bot_token)
        .json(&json!({
            "channel": channel,
            "user": user,
            "text": text,
        }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    if !response.ok {
        tracing::warn!(
            "slack chat.postEphemeral failed: {}",
            response.error.unwrap_or_else(|| "unknown_error".to_owned())
        );
    }
    Ok(())
}

pub async fn list_users(
    client: &Client,
    api_base_url: &str,
    bot_token: &str,
) -> Result<Vec<SlackUser>, GatewayError> {
    let response: SlackUsersListResponse = client
        .get(method_url(api_base_url, "users.list"))
        .bearer_auth(bot_token)
        .query(&[("limit", "200")])
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    if !response.ok {
        return Err(slack_api_error("users.list", response.error));
    }
    let members = response
        .members
        .unwrap_or_default()
        .into_iter()
        .filter(|u| !u.is_bot && u.id != "USLACKBOT")
        .collect();
    Ok(members)
}

fn method_url(api_base_url: &str, method: &str) -> String {
    format!("{}/{}", api_base_url.trim_end_matches('/'), method)
}

fn slack_api_error(method: &str, error: Option<String>) -> GatewayError {
    GatewayError::SandboxError(format!(
        "slack {method} failed: {}",
        error.unwrap_or_else(|| "unknown_error".to_owned())
    ))
}

fn truncate(text: &str) -> String {
    const MAX_CHARS: usize = 30_000;
    text.chars().take(MAX_CHARS).collect()
}
