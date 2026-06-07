use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::broadcast;

use crate::{
    agents::{events as agent_events, runs::AgentRunStatus},
    errors::GatewayError,
    proxy::state::AppState,
    sdk::agents::{AgentEvent, AgentEventStream},
};

use super::{
    reply_format::{runtime_status, runtime_text, slack_mrkdwn},
    reply_storage::{closed_text, final_text, persisted_assistant_text_after},
    types::SlackIncomingMessage,
    web_api,
};

pub(super) struct SlackReply<'a> {
    state: &'a AppState,
    pool: &'a PgPool,
    bot_token: &'a str,
    channel: &'a str,
    thread_ts: &'a str,
    ts: Option<String>,
    username: &'a str,
    session_id: &'a str,
    baseline_seq: i32,
    text: String,
    since_update: tokio::time::Instant,
}

pub(super) struct SlackReplyParams<'a> {
    pub state: &'a AppState,
    pub pool: &'a PgPool,
    pub bot_token: &'a str,
    pub message: &'a SlackIncomingMessage,
    pub username: &'a str,
    pub ts: Option<String>,
    pub session_id: &'a str,
    pub baseline_seq: i32,
}

impl<'a> SlackReply<'a> {
    pub(super) fn new(params: SlackReplyParams<'a>) -> Self {
        Self {
            state: params.state,
            pool: params.pool,
            bot_token: params.bot_token,
            channel: &params.message.channel,
            thread_ts: &params.message.reply_thread_ts,
            ts: params.ts,
            username: params.username,
            session_id: params.session_id,
            baseline_seq: params.baseline_seq,
            text: String::new(),
            since_update: tokio::time::Instant::now(),
        }
    }

    pub(super) async fn run(
        &mut self,
        mut rx: broadcast::Receiver<String>,
    ) -> Result<(), GatewayError> {
        loop {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Ok(line)) => {
                    if self.apply_line(&line).await? {
                        return Ok(());
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => {
                    if self.finish_if_terminal().await? {
                        return Ok(());
                    }
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => return self.finish_closed().await,
            }
        }
    }

    pub(super) async fn run_runtime(
        &mut self,
        mut stream: AgentEventStream,
    ) -> Result<(), GatewayError> {
        loop {
            match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
                Ok(Some(Ok(event))) => {
                    if self.handle_runtime_event(event).await? {
                        return Ok(());
                    }
                }
                Ok(Some(Err(error))) => {
                    self.update(&format!("Agent run failed: {error}")).await?;
                    return Err(GatewayError::SandboxError(error.to_string()));
                }
                Ok(None) => return self.finish_closed().await,
                Err(_) => {
                    if self.finish_if_terminal().await? {
                        return Ok(());
                    }
                }
            }
        }
    }

    pub(super) async fn finish_start_error(&mut self, message: &str) -> Result<(), GatewayError> {
        self.update(message).await
    }

    pub(super) async fn replace_text(&mut self, message: &str) -> Result<(), GatewayError> {
        self.text = message.to_owned();
        self.update(message).await
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

    async fn handle_runtime_event(&mut self, event: AgentEvent) -> Result<bool, GatewayError> {
        match event.event_type.as_str() {
            "agent.message"
            | "assistant_response"
            | "message.part.delta"
            | "message.part.updated"
            | "content_block_delta" => {
                if let Some(text) = runtime_text(&event) {
                    self.handle_text_delta(&text).await?;
                }
                Ok(false)
            }
            "session.status_idle" | "session.idle" => self.finish_success().await,
            "session.status" => match runtime_status(&event) {
                Some("idle") => self.finish_success().await,
                Some("error") | Some("failed") => {
                    self.finish_error(&Value::Object(event.data)).await
                }
                _ => Ok(false),
            },
            "session.error" | "error" => self.finish_error(&Value::Object(event.data)).await,
            _ => Ok(false),
        }
    }

    async fn handle_delta(&mut self, properties: &Value) -> Result<bool, GatewayError> {
        if let Some(delta) = properties.get("delta").and_then(Value::as_str) {
            self.handle_text_delta(delta).await?;
        }
        Ok(false)
    }

    async fn handle_text_delta(&mut self, delta: &str) -> Result<(), GatewayError> {
        self.text.push_str(delta);
        if self.ts.is_some()
            && self.since_update.elapsed() >= Duration::from_secs(1)
            && !self.text.is_empty()
        {
            let text = self.text.clone();
            self.update(&text).await?;
            self.since_update = tokio::time::Instant::now();
        }
        Ok(())
    }

    async fn finish_error(&mut self, properties: &Value) -> Result<bool, GatewayError> {
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

    async fn finish_if_terminal(&mut self) -> Result<bool, GatewayError> {
        if let Some(text) = self.persisted_text().await? {
            self.update(&text).await?;
            return Ok(true);
        }
        let Some(run) = self.state.agent_runs.get_run(self.session_id) else {
            return Ok(false);
        };
        match run.status {
            AgentRunStatus::Completed => {
                let text = final_text(&self.text);
                self.update(&text).await?;
                Ok(true)
            }
            AgentRunStatus::Failed | AgentRunStatus::TimedOut => {
                let text = run.error.as_deref().unwrap_or("Agent run failed.");
                self.update(text).await?;
                Ok(true)
            }
            AgentRunStatus::Starting | AgentRunStatus::Running => Ok(false),
        }
    }

    async fn persisted_text(&self) -> Result<Option<String>, GatewayError> {
        persisted_assistant_text_after(self.pool, self.session_id, self.baseline_seq).await
    }

    async fn update(&mut self, text: &str) -> Result<(), GatewayError> {
        let text = slack_mrkdwn(text);
        match self.ts.as_deref() {
            Some(ts) => {
                web_api::update_message(
                    &self.state.http,
                    &self.state.config.slack.api_base_url,
                    self.bot_token,
                    self.channel,
                    ts,
                    &text,
                )
                .await
            }
            None => {
                self.ts = Some(
                    web_api::post_message_as(
                        &self.state.http,
                        &self.state.config.slack.api_base_url,
                        self.bot_token,
                        self.channel,
                        self.thread_ts,
                        &text,
                        Some(self.username),
                    )
                    .await?,
                );
                Ok(())
            }
        }
    }
}

fn event_payload(line: &str) -> Option<(String, Value)> {
    let data = line.strip_prefix("data: ")?;
    let payload: Value = serde_json::from_str(data.trim()).ok()?;
    Some((
        payload.get("type")?.as_str()?.to_owned(),
        payload.get("properties")?.clone(),
    ))
}
