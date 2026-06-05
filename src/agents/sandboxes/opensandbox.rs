use std::collections::HashMap;
use std::time::{Duration, Instant};

mod sse;

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    agents::{
        config::OpenSandboxParams,
        sandboxes::{boxed_stream, AgentOutputStream, SandboxCommand},
    },
    errors::GatewayError,
};

use self::sse::ExecdEventDecoder;

pub const PROVIDER: &str = "opensandbox";

const API_KEY_HEADER: &str = "OPEN-SANDBOX-API-KEY";
const EXECD_TOKEN_HEADER: &str = "X-EXECD-ACCESS-TOKEN";

#[derive(Debug, Clone)]
pub struct OpenSandboxClient {
    http: Client,
    settings: OpenSandboxParams,
}

#[derive(Debug, Clone)]
pub struct OpenSandbox {
    pub id: String,
    execd_base_url: String,
    execd_headers: HashMap<String, String>,
}

impl OpenSandboxClient {
    pub fn new(http: Client, settings: OpenSandboxParams) -> Self {
        Self { http, settings }
    }

    pub async fn create(&self, run_id: &str) -> Result<OpenSandbox, GatewayError> {
        let api_key = self.api_key()?;
        let response = self
            .http
            .post(format!("{}/sandboxes", self.api_base()))
            .header(API_KEY_HEADER, api_key)
            .json(&CreateSandboxRequest {
                image: ImageSpec {
                    uri: &self.settings.image,
                },
                entrypoint: &self.settings.entrypoint,
                timeout: self.settings.timeout_seconds,
                resource_limits: ResourceLimits {
                    cpu: &self.settings.cpu,
                    memory: &self.settings.memory,
                },
                env: &self.settings.envs,
                secure_access: self.settings.secure_access,
                metadata: SandboxMetadata {
                    name: run_id,
                    run_id,
                },
            })
            .send()
            .await
            .map_err(GatewayError::Sandbox)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GatewayError::SandboxError(format!(
                "OpenSandbox create sandbox failed with status {status}: {body}"
            )));
        }

        let created: SandboxView = response.json().await.map_err(GatewayError::Sandbox)?;
        let id = created.id;
        self.wait_until_running(&id).await?;
        let (execd_base_url, execd_headers) = self.resolve_execd_endpoint(&id).await?;

        Ok(OpenSandbox {
            id,
            execd_base_url,
            execd_headers,
        })
    }

    pub async fn start_command(
        &self,
        sandbox: &OpenSandbox,
        command: SandboxCommand,
    ) -> Result<AgentOutputStream, GatewayError> {
        let command = command_with_workspace(&self.settings.workspace_dir, &command.command);
        let mut request = self
            .http
            .post(format!("{}/command", sandbox.execd_base_url))
            .header("Connect-Protocol-Version", "1")
            .json(&RunCommandRequest {
                command: &command,
                cwd: &self.settings.workspace_dir,
                background: false,
                envs: &self.settings.envs,
            });
        for (key, value) in &sandbox.execd_headers {
            request = request.header(key, value);
        }

        let response = request.send().await.map_err(GatewayError::Sandbox)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GatewayError::SandboxError(format!(
                "OpenSandbox run command failed with status {status}: {body}"
            )));
        }

        let mut decoder = ExecdEventDecoder::default();
        let stream = response
            .bytes_stream()
            .map(move |bytes| {
                bytes
                    .map_err(GatewayError::Sandbox)
                    .map(|bytes| decoder.decode(bytes))
            })
            .map_ok(|chunks| stream::iter(chunks.into_iter().map(Ok)))
            .try_flatten();

        Ok(boxed_stream(stream))
    }

    pub async fn terminate(&self, sandbox_id: &str) -> Result<(), GatewayError> {
        let api_key = self.api_key()?;
        let response = self
            .http
            .delete(format!("{}/sandboxes/{}", self.api_base(), sandbox_id))
            .header(API_KEY_HEADER, api_key)
            .send()
            .await
            .map_err(GatewayError::Sandbox)?;

        if !response.status().is_success() && response.status().as_u16() != 404 {
            return Err(GatewayError::SandboxError(format!(
                "OpenSandbox terminate sandbox failed with status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn wait_until_running(&self, sandbox_id: &str) -> Result<(), GatewayError> {
        let api_key = self.api_key()?;
        let deadline = Instant::now() + Duration::from_secs(self.settings.ready_timeout_seconds);
        let poll = Duration::from_millis(self.settings.poll_interval_ms.max(1));

        loop {
            let response = self
                .http
                .get(format!("{}/sandboxes/{}", self.api_base(), sandbox_id))
                .header(API_KEY_HEADER, api_key)
                .send()
                .await
                .map_err(GatewayError::Sandbox)?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(GatewayError::SandboxError(format!(
                    "OpenSandbox get sandbox failed with status {status}: {body}"
                )));
            }

            let view: SandboxView = response.json().await.map_err(GatewayError::Sandbox)?;
            match view.status.state.as_str() {
                "Running" => return Ok(()),
                "Failed" | "Terminated" | "Stopping" => {
                    return Err(GatewayError::SandboxError(format!(
                        "OpenSandbox sandbox entered {} before becoming ready: {}",
                        view.status.state,
                        view.status.message.unwrap_or_default()
                    )));
                }
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(GatewayError::SandboxError(format!(
                    "OpenSandbox sandbox {sandbox_id} did not become ready within {}s",
                    self.settings.ready_timeout_seconds
                )));
            }
            tokio::time::sleep(poll).await;
        }
    }

    async fn resolve_execd_endpoint(
        &self,
        sandbox_id: &str,
    ) -> Result<(String, HashMap<String, String>), GatewayError> {
        let api_key = self.api_key()?;
        let response = self
            .http
            .get(format!(
                "{}/sandboxes/{}/endpoints/{}",
                self.api_base(),
                sandbox_id,
                self.settings.execd_port
            ))
            .header(API_KEY_HEADER, api_key)
            .send()
            .await
            .map_err(GatewayError::Sandbox)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GatewayError::SandboxError(format!(
                "OpenSandbox get endpoint failed with status {status}: {body}"
            )));
        }

        let endpoint: EndpointView = response.json().await.map_err(GatewayError::Sandbox)?;
        let base_url = ensure_scheme(&endpoint.endpoint, self.scheme());

        let mut headers = endpoint.headers.unwrap_or_default();
        if let Some(token) = self
            .settings
            .execd_access_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
        {
            headers
                .entry(EXECD_TOKEN_HEADER.to_owned())
                .or_insert_with(|| token.to_owned());
        }

        Ok((base_url, headers))
    }

    fn api_base(&self) -> &str {
        self.settings.api_base.trim_end_matches('/')
    }

    fn scheme(&self) -> &'static str {
        if self.settings.api_base.starts_with("https://") {
            "https"
        } else {
            "http"
        }
    }

    fn api_key(&self) -> Result<&str, GatewayError> {
        self.settings
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| {
                GatewayError::InvalidConfig(
                    "general_settings.opensandbox_sandbox_params.api_key is required".to_owned(),
                )
            })
    }
}

fn ensure_scheme(endpoint: &str, scheme: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_owned()
    } else {
        format!("{scheme}://{endpoint}")
    }
}

fn command_with_workspace(workspace_dir: &str, command: &str) -> String {
    format!(
        "mkdir -p {} && cd {} && {}",
        shell_quote(workspace_dir),
        shell_quote(workspace_dir),
        command
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Serialize)]
struct CreateSandboxRequest<'a> {
    image: ImageSpec<'a>,
    entrypoint: &'a [String],
    timeout: u64,
    #[serde(rename = "resourceLimits")]
    resource_limits: ResourceLimits<'a>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    env: &'a HashMap<String, String>,
    #[serde(rename = "secureAccess")]
    secure_access: bool,
    metadata: SandboxMetadata<'a>,
}

#[derive(Serialize)]
struct ImageSpec<'a> {
    uri: &'a str,
}

#[derive(Serialize)]
struct ResourceLimits<'a> {
    cpu: &'a str,
    memory: &'a str,
}

#[derive(Serialize)]
struct SandboxMetadata<'a> {
    name: &'a str,
    run_id: &'a str,
}

#[derive(Serialize)]
struct RunCommandRequest<'a> {
    command: &'a str,
    cwd: &'a str,
    background: bool,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    envs: &'a HashMap<String, String>,
}

#[derive(Deserialize)]
struct SandboxView {
    id: String,
    #[serde(default)]
    status: SandboxStatus,
}

#[derive(Deserialize, Default)]
struct SandboxStatus {
    #[serde(default)]
    state: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct EndpointView {
    endpoint: String,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_scheme_adds_default_scheme() {
        assert_eq!(
            ensure_scheme("endpoint.example.com/sandboxes/a/port/44772", "https"),
            "https://endpoint.example.com/sandboxes/a/port/44772"
        );
    }

    #[test]
    fn ensure_scheme_keeps_existing() {
        assert_eq!(
            ensure_scheme("http://localhost:8080/proxy/", "https"),
            "http://localhost:8080/proxy"
        );
    }

    #[test]
    fn command_runs_inside_workspace() {
        let command = command_with_workspace("/workspace", "echo hi");
        assert_eq!(command, "mkdir -p '/workspace' && cd '/workspace' && echo hi");
    }
}
