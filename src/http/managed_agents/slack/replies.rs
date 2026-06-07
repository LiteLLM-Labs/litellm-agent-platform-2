use std::sync::Arc;

use sqlx::PgPool;
use tracing::warn;

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    http::sessions::{enqueue_prompt_text, runtime_event_stream_for_session},
    proxy::state::AppState,
};

use super::{
    config::{bot_token_key, load_secret},
    reply_lock::SlackPromptLock,
    reply_storage::last_message_seq,
    reply_stream::SlackReply,
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
    if let Err(error) = web_api::add_reaction(
        &state.http,
        &state.config.slack.api_base_url,
        &bot_token,
        &message.channel,
        &message.reply_thread_ts,
        "eyes",
    )
    .await
    {
        warn!("slack eyes reaction failed: {error}");
    }
    let _lock = SlackPromptLock::acquire(&state.keyed_locks, &session_id).await;
    run_locked_slack_prompt(state, &pool, agent, message, session_id, bot_token).await
}

async fn run_locked_slack_prompt(
    state: Arc<AppState>,
    pool: &PgPool,
    agent: ManagedAgentRow,
    message: SlackIncomingMessage,
    session_id: String,
    bot_token: String,
) -> Result<(), GatewayError> {
    let baseline_seq = last_message_seq(pool, &session_id).await?;
    let runtime_stream = runtime_event_stream_for_session(&state, pool, &session_id)
        .await
        .ok();
    let event_stream = state.agent_runs.event_stream();
    let placeholder = post_placeholder(&state, &bot_token, &message).await;
    let mut reply = SlackReply::new(
        &state,
        pool,
        &bot_token,
        &message,
        placeholder,
        &session_id,
        baseline_seq,
    );
    enqueue_or_report(&state, pool, &message, &mut reply, &session_id, &agent).await?;
    if let Some(stream) = runtime_stream {
        return match reply.run_runtime(stream).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = format!("Agent run failed: {error}");
                if let Err(update_error) = reply.finish_start_error(&message).await {
                    warn!("slack failure update failed: {update_error}");
                }
                Err(error)
            }
        };
    }
    match reply.run(event_stream.rx).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = format!("Agent run failed: {error}");
            if let Err(update_error) = reply.finish_start_error(&message).await {
                warn!("slack failure update failed: {update_error}");
            }
            Err(error)
        }
    }
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
        &message.reply_thread_ts,
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
    message: &SlackIncomingMessage,
    reply: &mut SlackReply<'_>,
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
        reply.finish_start_error(&error.to_string()).await?;
        return Err(error);
    }
    Ok(())
}

pub(super) async fn post_denial_ephemeral(
    state: &AppState,
    agent: &ManagedAgentRow,
    config: &SlackAgentConfig,
    payload: &serde_json::Value,
    user_id: &str,
) {
    if user_id.is_empty() {
        return;
    }
    let Ok(bot_token) = load_secret(state, &bot_token_key(&agent.id, config)).await else {
        return;
    };
    let channel_id = payload
        .get("event")
        .and_then(|e| e.get("channel"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if !channel_id.is_empty() {
        let _ = web_api::post_ephemeral(
            &state.http,
            &state.config.slack.api_base_url,
            &bot_token,
            channel_id,
            user_id,
            "You don't have permission to use this agent.",
        )
        .await;
    }
}
