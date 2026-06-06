use std::sync::Arc;

use sqlx::PgPool;
use tracing::warn;

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow, errors::GatewayError,
    http::sessions::enqueue_prompt_text, proxy::state::AppState,
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
    reply.run(event_stream.rx).await
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
