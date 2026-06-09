use axum::http::HeaderName;
use reqwest::{Method, Url};
use serde_json::{json, Map, Value};
use sqlx::PgPool;

use crate::{
    db::{credentials, managed_agents::registry},
    errors::GatewayError,
    proxy::{credential_crypto, state::AppState},
};

use super::{required_str, vault_key_names};

pub async fn proxy_request(
    state: &AppState,
    pool: &PgPool,
    agent_id: &str,
    arguments: Value,
) -> Result<Value, GatewayError> {
    let agent = registry::repository::get(pool, agent_id)
        .await?
        .ok_or_else(|| GatewayError::UnknownAgent(agent_id.to_owned()))?;
    let key_name = required_str(&arguments, "key")?;
    if !vault_key_names(&agent.vault_keys)
        .iter()
        .any(|key| key == key_name)
    {
        return Ok(json!({
            "isError": true,
            "message": "vault key is not attached to this agent"
        }));
    }
    let credential = resolve_plaintext_credential(state, pool, &agent, key_name).await?;
    let method = request_method(&arguments)?;
    let url = request_url(required_str(&arguments, "url")?)?;
    let mut request = state.http.request(method, url);
    request = apply_headers(request, arguments.get("headers"))?;
    request = apply_auth(request, &arguments, &credential)?;
    if let Some(body) = arguments.get("body") {
        request = request.json(body);
    }
    let response = request.send().await.map_err(GatewayError::Upstream)?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let text = response.text().await.map_err(GatewayError::Upstream)?;
    Ok(json!({
        "status": status,
        "content_type": content_type,
        "body": redact(&text, &credential)
    }))
}

async fn resolve_plaintext_credential(
    state: &AppState,
    pool: &PgPool,
    agent: &registry::schema::ManagedAgentRow,
    key_name: &str,
) -> Result<String, GatewayError> {
    let owner_id = agent.owner_id.as_deref().unwrap_or("");
    let encrypted = credentials::resolve_vault_key(pool, key_name, owner_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound(format!("vault key '{key_name}' not found")))?;
    let enc_key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;
    credential_crypto::decrypt_value(&encrypted, &enc_key)
}

fn request_method(arguments: &Value) -> Result<Method, GatewayError> {
    let method = arguments
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    match method.as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        _ => Err(GatewayError::InvalidJsonMessage(
            "method must be GET, POST, PUT, PATCH, or DELETE".to_owned(),
        )),
    }
}

fn request_url(raw: &str) -> Result<Url, GatewayError> {
    let url = Url::parse(raw)
        .map_err(|_| GatewayError::InvalidJsonMessage("url is invalid".to_owned()))?;
    if url.scheme() == "https" || is_local_http(&url) {
        return Ok(url);
    }
    Err(GatewayError::InvalidJsonMessage(
        "url must be https, except localhost http URLs during development".to_owned(),
    ))
}

fn is_local_http(url: &Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    )
}

fn apply_headers(
    mut request: reqwest::RequestBuilder,
    headers: Option<&Value>,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    let Some(headers) = headers.and_then(Value::as_object) else {
        return Ok(request);
    };
    for (name, value) in headers {
        let value = value.as_str().ok_or_else(|| {
            GatewayError::InvalidJsonMessage("headers values must be strings".to_owned())
        })?;
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| GatewayError::InvalidJsonMessage("invalid header name".to_owned()))?;
        request = request.header(name, value);
    }
    Ok(request)
}

fn apply_auth(
    request: reqwest::RequestBuilder,
    arguments: &Value,
    credential: &str,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    let auth = arguments.get("auth").and_then(Value::as_object);
    match auth
        .and_then(|auth| auth.get("type"))
        .and_then(Value::as_str)
    {
        None | Some("bearer") => Ok(request.bearer_auth(credential)),
        Some("api_key_header") => {
            let header = auth_header(auth, "x-api-key")?;
            Ok(request.header(header, credential))
        }
        Some("header") => {
            let header = auth_header(auth, "")?;
            Ok(request.header(header, credential))
        }
        Some("query") => {
            let query = auth
                .and_then(|auth| auth.get("query"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .ok_or_else(|| {
                    GatewayError::InvalidJsonMessage("auth.query is required".to_owned())
                })?;
            Ok(request.query(&[(query, credential)]))
        }
        Some(kind) => Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported auth type: {kind}"
        ))),
    }
}

fn auth_header(
    auth: Option<&Map<String, Value>>,
    default: &str,
) -> Result<HeaderName, GatewayError> {
    let header = auth
        .and_then(|auth| auth.get("header"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|header| !header.is_empty())
        .unwrap_or(default);
    if header.is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "auth.header is required".to_owned(),
        ));
    }
    HeaderName::from_bytes(header.as_bytes())
        .map_err(|_| GatewayError::InvalidJsonMessage("invalid auth header".to_owned()))
}

fn redact(text: &str, credential: &str) -> String {
    if credential.is_empty() {
        return text.to_owned();
    }
    text.replace(credential, "[REDACTED_VAULT_CREDENTIAL]")
}
