use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::broadcast;

use crate::{
    agents::{events as agent_events, runs::AgentRunStatus},
    errors::GatewayError,
    proxy::state::AppState,
};

use super::{
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
    session_id: &'a str,
    baseline_seq: i32,
    text: String,
    since_update: tokio::time::Instant,
}

impl<'a> SlackReply<'a> {
    pub(super) fn new(
        state: &'a AppState,
        pool: &'a PgPool,
        bot_token: &'a str,
        message: &'a SlackIncomingMessage,
        ts: Option<String>,
        session_id: &'a str,
        baseline_seq: i32,
    ) -> Self {
        Self {
            state,
            pool,
            bot_token,
            channel: &message.channel,
            thread_ts: &message.reply_thread_ts,
            ts,
            session_id,
            baseline_seq,
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

    pub(super) async fn finish_start_error(&mut self, message: &str) -> Result<(), GatewayError> {
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

    async fn handle_delta(&mut self, properties: &Value) -> Result<bool, GatewayError> {
        if let Some(delta) = properties.get("delta").and_then(Value::as_str) {
            self.text.push_str(delta);
        }
        if self.ts.is_some()
            && self.since_update.elapsed() >= Duration::from_secs(1)
            && !self.text.is_empty()
        {
            let text = self.text.clone();
            self.update(&text).await?;
            self.since_update = tokio::time::Instant::now();
        }
        Ok(false)
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
        match self.ts.as_deref() {
            Some(ts) => {
                web_api::update_message(
                    &self.state.http,
                    &self.state.config.slack.api_base_url,
                    self.bot_token,
                    self.channel,
                    ts,
                    text,
                )
                .await
            }
            None => {
                self.ts = Some(
                    web_api::post_message(
                        &self.state.http,
                        &self.state.config.slack.api_base_url,
                        self.bot_token,
                        self.channel,
                        self.thread_ts,
                        text,
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
