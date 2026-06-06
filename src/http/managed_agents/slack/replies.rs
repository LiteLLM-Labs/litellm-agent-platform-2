use std::{sync::Arc, time::Duration};

use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::warn;

use crate::{
    agents::events as agent_events, db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError, http::sessions::enqueue_prompt_text, proxy::state::AppState,
};

use super::{
    config::{bot_token_key, load_secret},
    reply_storage::{closed_text, final_text, persisted_assistant_text},
    types::{SlackAgentConfig, SlackIncomingMessage},
    web_api,
};

pub(super) fn spawn_slack_prompt(
    state: Arc<AppState>,
    pool: PgPool,
    agent: ManagedAgentRow,
    config: SlackAgentConfig,
    message: SlackIncomingMessage,
    session_id: String,
) {
    tokio::spawn(async move {
        if let Err(error) = run_slack_prompt(state, pool, agent, config, message, session_id).await
        {
            warn!("slack prompt failed: {error}");
        }
    });
}

async fn run_slack_prompt(
    state: Arc<AppState>,
    pool: PgPool,
    agent: ManagedAgentRow,
    config: SlackAgentConfig,
    message: SlackIncomingMessage,
    session_id: String,
) -> Result<(), GatewayError> {
    let bot_token = load_secret(&state, &bot_token_key(&agent.id, &config)).await?;
    let event_stream = state.agent_runs.event_stream();
    let placeholder = post_placeholder(&state, &bot_token, &message).await;
    enqueue_or_report(
        &state,
        &pool,
        &bot_token,
        &message,
        placeholder.as_deref(),
        &session_id,
        &agent,
    )
    .await?;
    if let Some(ts) = placeholder {
        let mut reply = SlackReply::new(
            &state,
            &pool,
            &bot_token,
            &message.channel,
            &ts,
            &session_id,
        );
        reply.run(event_stream.rx).await?;
    }
    Ok(())
}

async fn post_placeholder(
    state: &AppState,
    bot_token: &str,
    message: &SlackIncomingMessage,
) -> Option<String> {
    match web_api::post_message(
        &state.http,
        &state.config.slack.api_base_url,
        bot_token,
        &message.channel,
        &message.thread_ts,
        "_Thinking..._",
    )
    .await
    {
        Ok(ts) => Some(ts),
        Err(error) => {
            warn!("slack placeholder failed: {error}");
            None
        }
    }
}

async fn enqueue_or_report(
    state: &Arc<AppState>,
    pool: &PgPool,
    bot_token: &str,
    message: &SlackIncomingMessage,
    placeholder_ts: Option<&str>,
    session_id: &str,
    agent: &ManagedAgentRow,
) -> Result<(), GatewayError> {
    let result = enqueue_prompt_text(
        state.clone(),
        pool.clone(),
        session_id,
        message.prompt.clone(),
        agent.model.clone(),
    )
    .await;
    if let Err(error) = result {
        if let Some(ts) = placeholder_ts {
            update_message(state, bot_token, &message.channel, ts, &error.to_string()).await?;
        }
        return Err(error);
    }
    Ok(())
}

struct SlackReply<'a> {
    state: &'a AppState,
    pool: &'a PgPool,
    bot_token: &'a str,
    channel: &'a str,
    ts: &'a str,
    session_id: &'a str,
    text: String,
    since_update: tokio::time::Instant,
}

impl<'a> SlackReply<'a> {
    fn new(
        state: &'a AppState,
        pool: &'a PgPool,
        bot_token: &'a str,
        channel: &'a str,
        ts: &'a str,
        session_id: &'a str,
    ) -> Self {
        Self {
            state,
            pool,
            bot_token,
            channel,
            ts,
            session_id,
            text: String::new(),
            since_update: tokio::time::Instant::now(),
        }
    }

    async fn run(&mut self, mut rx: broadcast::Receiver<String>) -> Result<(), GatewayError> {
        loop {
            match rx.recv().await {
                Ok(line) => {
                    if self.apply_line(&line).await? {
                        return Ok(());
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if self.refresh_terminal_from_snapshot().await? {
                        return Ok(());
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return self.finish_closed().await,
            }
        }
    }

    async fn apply_line(&mut self, line: &str) -> Result<bool, GatewayError> {
        let Some((event_type, properties)) = event_payload(line) else {
            return Ok(false);
        };
        if properties.get("sessionID").and_then(Value::as_str) != Some(self.session_id) {
            return Ok(false);
        }
        self.handle_event(&event_type, &properties).await
    }

    async fn handle_event(
        &mut self,
        event_type: &str,
        properties: &Value,
    ) -> Result<bool, GatewayError> {
        match event_type {
            agent_events::MESSAGE_PART_DELTA => self.handle_delta(properties).await,
            agent_events::SESSION_ERROR => self.finish_error(properties).await,
            agent_events::SESSION_IDLE => self.finish_success().await,
            _ => Ok(false),
        }
    }

    async fn handle_delta(&mut self, properties: &Value) -> Result<bool, GatewayError> {
        if let Some(delta) = properties.get("delta").and_then(Value::as_str) {
            self.text.push_str(delta);
        }
        if self.since_update.elapsed() >= Duration::from_secs(1) && !self.text.is_empty() {
            self.update(&self.text).await?;
            self.since_update = tokio::time::Instant::now();
        }
        Ok(false)
    }

    async fn finish_error(&self, properties: &Value) -> Result<bool, GatewayError> {
        let message = properties
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Agent run failed.");
        self.update(message).await?;
        Ok(true)
    }

    async fn finish_success(&mut self) -> Result<bool, GatewayError> {
        let text = self
            .persisted_text()
            .await?
            .unwrap_or_else(|| final_text(&self.text));
        self.update(&text).await?;
        Ok(true)
    }

    async fn finish_closed(&mut self) -> Result<(), GatewayError> {
        let text = self
            .persisted_text()
            .await?
            .unwrap_or_else(|| closed_text(&self.text));
        self.update(&text).await
    }

    async fn refresh_terminal_from_snapshot(&mut self) -> Result<bool, GatewayError> {
        for line in self.state.agent_runs.event_stream().snapshot {
            let Some((event_type, properties)) = event_payload(&line) else {
                continue;
            };
            if properties.get("sessionID").and_then(Value::as_str) != Some(self.session_id) {
                continue;
            }
            if matches!(
                event_type.as_str(),
                agent_events::SESSION_ERROR | agent_events::SESSION_IDLE
            ) {
                return self.handle_event(&event_type, &properties).await;
            }
        }
        Ok(false)
    }

    async fn persisted_text(&self) -> Result<Option<String>, GatewayError> {
        persisted_assistant_text(self.pool, self.session_id).await
    }

    async fn update(&self, text: &str) -> Result<(), GatewayError> {
        web_api::update_message(
            &self.state.http,
            &self.state.config.slack.api_base_url,
            self.bot_token,
            self.channel,
            self.ts,
            text,
        )
        .await
    }
}

async fn update_message(
    state: &AppState,
    bot_token: &str,
    channel: &str,
    ts: &str,
    text: &str,
) -> Result<(), GatewayError> {
    web_api::update_message(
        &state.http,
        &state.config.slack.api_base_url,
        bot_token,
        channel,
        ts,
        text,
    )
    .await
}

fn event_payload(line: &str) -> Option<(String, Value)> {
    let data = line.strip_prefix("data: ")?;
    let payload: Value = serde_json::from_str(data.trim()).ok()?;
    Some((
        payload.get("type")?.as_str()?.to_owned(),
        payload.get("properties")?.clone(),
    ))
}
