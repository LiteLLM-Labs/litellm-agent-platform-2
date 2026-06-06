use axum::http::StatusCode;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;

use super::super::{read_events_until_completed, request_json, request_with_headers, AppFixture};

pub async fn exercise_slack(fixture: &AppFixture, agent_id: &str) {
    save_slack_secrets(fixture, agent_id).await;
    configure_agent_slack(fixture, agent_id).await;
    assert_url_verification(fixture, agent_id).await;
    let session_id = send_app_mention(fixture, agent_id).await;
    let events = read_events_until_completed(fixture.app.clone(), "/event", &session_id).await;
    assert!(events.contains("\"type\":\"message.part.delta\""));
    assert!(events.contains("\"delta\":\"hello \""));
    assert_slack_api_called(fixture, "/chat.postMessage").await;
    assert_slack_api_called(fixture, "/chat.update").await;
    assert_interactivity_accepts_approval(fixture, agent_id).await;
}

async fn save_slack_secrets(fixture: &AppFixture, agent_id: &str) {
    for (key, value) in [
        (format!("SLACK_{agent_id}_SIGNING_SECRET"), "slack-secret"),
        (format!("SLACK_{agent_id}_BOT_TOKEN"), "xoxb-test"),
    ] {
        request_json(
            fixture.app.clone(),
            "POST",
            "/api/vault/default",
            Some(json!({ "key": key, "value": value })),
        )
        .await;
    }
    let vault = request_json(fixture.app.clone(), "GET", "/api/vault/default", None).await;
    let keys = vault["keys"].as_array().unwrap();
    assert!(keys
        .iter()
        .any(|entry| entry["key"] == format!("SLACK_{agent_id}_SIGNING_SECRET")));
}

async fn configure_agent_slack(fixture: &AppFixture, agent_id: &str) {
    request_json(
        fixture.app.clone(),
        "PATCH",
        &format!("/api/agents/{agent_id}"),
        Some(json!({
            "config": {
                "slack": {
                    "status": "connected",
                    "signing_secret_key": format!("SLACK_{agent_id}_SIGNING_SECRET"),
                    "bot_token_key": format!("SLACK_{agent_id}_BOT_TOKEN")
                }
            }
        })),
    )
    .await;
}

async fn assert_url_verification(fixture: &AppFixture, agent_id: &str) {
    let body = json!({
        "type": "url_verification",
        "challenge": "challenge-ok"
    })
    .to_string();
    let response = signed_json_request(
        fixture,
        &format!("/api/agents/{agent_id}/slack/events"),
        body,
        StatusCode::OK,
    )
    .await;
    assert_eq!(response, "challenge-ok");
}

async fn send_app_mention(fixture: &AppFixture, agent_id: &str) -> String {
    let body = json!({
        "type": "event_callback",
        "team_id": "T123",
        "api_app_id": "A123",
        "event_id": "Ev123",
        "event_time": now_seconds(),
        "event": {
            "type": "app_mention",
            "user": "U123",
            "text": "<@B123> say hello",
            "ts": "1712345678.000100",
            "channel": "C123",
            "event_ts": "1712345678.000100"
        }
    })
    .to_string();
    signed_json_request(
        fixture,
        &format!("/api/agents/{agent_id}/slack/events"),
        body,
        StatusCode::OK,
    )
    .await;
    wait_for_slack_session(&fixture.pool, agent_id, "C123", "1712345678.000100").await
}

async fn assert_interactivity_accepts_approval(fixture: &AppFixture, agent_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO "LiteLLM_ManagedAgentInboxItemsTable"
          (id, kind, title, status, created_at)
        VALUES ('slack_appr_1', 'approval', 'approve slack action', 'pending', 10)
        "#,
    )
    .execute(&fixture.pool)
    .await
    .unwrap();

    let payload = json!({
        "type": "block_actions",
        "actions": [{
            "action_id": "lap_approval_accept",
            "value": "slack_appr_1"
        }]
    })
    .to_string();
    let body = format!("payload={}", percent_encode(&payload));
    signed_request(
        fixture,
        &format!("/api/agents/{agent_id}/slack/interactivity"),
        body,
        "application/x-www-form-urlencoded",
        StatusCode::OK,
    )
    .await;
    let status: String = sqlx::query_scalar(
        r#"
        SELECT status
        FROM "LiteLLM_ManagedAgentInboxItemsTable"
        WHERE id = 'slack_appr_1'
        "#,
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(status, "accepted");
}

async fn wait_for_slack_session(
    pool: &PgPool,
    agent_id: &str,
    channel_id: &str,
    thread_ts: &str,
) -> String {
    for _ in 0..20 {
        if let Some(session_id) = sqlx::query_scalar::<_, String>(
            r#"
            SELECT session_id
            FROM "LiteLLM_ManagedAgentSlackThreadSessionsTable"
            WHERE agent_id = $1 AND channel_id = $2 AND thread_ts = $3
            "#,
        )
        .bind(agent_id)
        .bind(channel_id)
        .bind(thread_ts)
        .fetch_optional(pool)
        .await
        .unwrap()
        {
            return session_id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("slack thread session was not created");
}

async fn assert_slack_api_called(fixture: &AppFixture, path: &str) {
    for _ in 0..20 {
        let requests = fixture.slack.received_requests().await.unwrap();
        if requests.iter().any(|request| request.url.path() == path) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("slack mock did not receive {path}");
}

async fn signed_json_request(
    fixture: &AppFixture,
    uri: &str,
    body: String,
    expected: StatusCode,
) -> String {
    signed_request(fixture, uri, body, "application/json", expected).await
}

async fn signed_request(
    fixture: &AppFixture,
    uri: &str,
    body: String,
    content_type: &str,
    expected: StatusCode,
) -> String {
    let timestamp = now_seconds();
    request_with_headers(
        fixture.app.clone(),
        "POST",
        uri,
        body.clone(),
        content_type,
        &[
            ("x-slack-request-timestamp", timestamp.to_string()),
            (
                "x-slack-signature",
                slack_signature(timestamp, body.as_bytes(), "slack-secret"),
            ),
        ],
        expected,
    )
    .await
}

fn slack_signature(timestamp: i64, body: &[u8], signing_secret: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes()).unwrap();
    mac.update(format!("v0:{timestamp}:").as_bytes());
    mac.update(body);
    format!("v0={}", lower_hex(&mac.finalize().into_bytes()))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            byte => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
