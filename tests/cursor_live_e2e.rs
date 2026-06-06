use std::{
    error::Error,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    AgentModel, AgentRuntime, CreateAgentParams, CreateSessionParams, Lap, LapConfig,
};
use tokio::time::{timeout, Duration};

#[tokio::test]
#[ignore = "requires CURSOR_API_KEY and creates a real Cursor Cloud Agent"]
async fn cursor_live_agent_session_stream_smoke() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("CURSOR_API_KEY")
        .map_err(|_| "CURSOR_API_KEY must be set to run the live Cursor smoke test")?;
    let model = std::env::var("CURSOR_MODEL").unwrap_or_else(|_| "default".to_owned());
    let client = Lap::new(LapConfig::cursor(api_key.clone()));
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::Cursor,
            name: format!("LAP SDK live smoke {suffix}"),
            model: AgentModel::from(model),
            system: "Reply with exactly: LAP cursor live smoke ok. Do not modify files.".to_owned(),
            description: None,
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            metadata: None,
        })
        .await?;

    let result = run_session_stream(&client, &agent.id).await;
    let cleanup = archive_cursor_agent(&api_key, &agent.id).await;
    if let Err(error) = cleanup {
        eprintln!(
            "failed to archive Cursor live smoke agent {}: {error}",
            agent.id
        );
    }

    result
}

async fn run_session_stream(client: &Lap, agent_id: &str) -> Result<(), Box<dyn Error>> {
    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: agent_id.to_owned().into(),
            environment_id: String::new(),
            title: "LAP SDK live smoke".to_owned(),
            lap_agent_runtime: Some(AgentRuntime::Cursor),
            metadata: None,
        })
        .await?;

    let mut stream = client
        .beta()
        .sessions()
        .events()
        .stream(&session.id)
        .await?;
    let mut saw_terminal_event = false;

    timeout(Duration::from_secs(180), async {
        while let Some(event) = stream.next().await {
            let event = event?;
            println!("cursor live event: {}", event.event_type);
            match event.event_type.as_str() {
                "session.status_idle" => {
                    saw_terminal_event = true;
                    break;
                }
                "session.error" => {
                    return Err(format!("Cursor live run failed: {:?}", event.data).into());
                }
                _ => {}
            }
        }
        Ok::<(), Box<dyn Error>>(())
    })
    .await??;

    if !saw_terminal_event {
        return Err("Cursor live stream ended without session.status_idle".into());
    }

    Ok(())
}

async fn archive_cursor_agent(api_key: &str, agent_id: &str) -> Result<(), Box<dyn Error>> {
    let response = reqwest::Client::new()
        .post(format!(
            "https://api.cursor.com/v1/agents/{agent_id}/archive"
        ))
        .bearer_auth(api_key)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("archive failed with {}", response.status()).into());
    }
    Ok(())
}
