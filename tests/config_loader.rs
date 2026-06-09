use std::io::Write;

use litellm_rust::proxy::config::{load_config, McpAuthType, McpTransport};
use tempfile::NamedTempFile;

fn write_config(contents: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file
}

#[test]
fn loads_litellm_style_anthropic_config() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"
model_list:
  - model_name: claude
    litellm_params:
      model: anthropic/claude-sonnet-4-5
      api_key: sk-ant-test
general_settings:
  master_key: sk-local
"#
    )
    .unwrap();

    let config = load_config(file.path()).unwrap();
    assert_eq!(config.model_list[0].model_name, "claude");
    assert_eq!(
        config.model_list[0].litellm_params.model,
        "anthropic/claude-sonnet-4-5"
    );
}

// --- MCP config (LiteLLM-compatible dict-keyed `mcp_servers`) -------------

#[test]
fn loads_litellm_dict_mcp_servers() {
    let file = write_config(
        r#"
mcp_servers:
  linear:
    url: https://mcp.linear.app/mcp
    auth_type: bearer_token
    auth_value: sk-linear
  docs:
    url: https://mcp.example.com/mcp
    auth_type: api_key
    auth_value: sk-docs
    static_headers:
      x-workspace: prod
    extra_headers: ["x-trace-id"]
general_settings:
  master_key: sk-local
"#,
    );

    let config = load_config(file.path()).unwrap();
    assert!(config.model_list.is_empty());
    assert_eq!(config.mcp_servers.len(), 2);

    let linear = &config.mcp_servers["linear"];
    assert_eq!(linear.url, "https://mcp.linear.app/mcp");
    assert_eq!(linear.auth_value.as_deref(), Some("sk-linear"));
    assert_eq!(linear.transport, McpTransport::Http);

    let docs = &config.mcp_servers["docs"];
    assert_eq!(docs.auth_type, McpAuthType::ApiKey);
    assert_eq!(docs.static_headers["x-workspace"], "prod");
    assert_eq!(docs.extra_headers, vec!["x-trace-id".to_owned()]);
}

#[test]
fn loads_proxy_base_url_from_mcp_servers_section() {
    let file = write_config(
        r#"
mcp_servers:
  proxy_base_url: https://gateway.example.com
  linear:
    url: https://mcp.linear.app/mcp
"#,
    );

    let config = load_config(file.path()).unwrap();
    assert_eq!(
        config.mcp_servers.proxy_base_url(),
        Some("https://gateway.example.com")
    );
    assert_eq!(config.mcp_servers.len(), 1);
    assert!(config.mcp_servers.contains_key("linear"));
}

#[test]
fn defaults_transport_http_and_auth_none() {
    let file = write_config(
        r#"
mcp_servers:
  deepwiki:
    url: https://mcp.deepwiki.com/mcp
"#,
    );
    let config = load_config(file.path()).unwrap();
    let s = &config.mcp_servers["deepwiki"];
    assert_eq!(s.transport, McpTransport::Http);
    assert_eq!(s.auth_type, McpAuthType::None);
    assert!(s.auth_value.is_none());
}

#[test]
fn expands_env_in_mcp_fields() {
    std::env::set_var("TEST_MCP_URL", "https://env.example.com/mcp");
    std::env::set_var("TEST_MCP_KEY", "sk-from-env");
    let file = write_config(
        r#"
mcp_servers:
  s:
    url: os.environ/TEST_MCP_URL
    auth_type: bearer_token
    auth_value: os.environ/TEST_MCP_KEY
"#,
    );
    let config = load_config(file.path()).unwrap();
    let s = &config.mcp_servers["s"];
    assert_eq!(s.url, "https://env.example.com/mcp");
    assert_eq!(s.auth_value.as_deref(), Some("sk-from-env"));
}

#[test]
fn expands_proxy_base_url_from_mcp_servers_section() {
    std::env::set_var(
        "TEST_LITELLM_PROXY_BASE_URL",
        "https://proxy-env.example.com",
    );
    let file = write_config(
        r#"
mcp_servers:
  proxy_base_url: os.environ/TEST_LITELLM_PROXY_BASE_URL
  s:
    url: https://mcp.example.com/mcp
"#,
    );

    let config = load_config(file.path()).unwrap();
    assert_eq!(
        config.mcp_servers.proxy_base_url(),
        Some("https://proxy-env.example.com")
    );
    std::env::remove_var("TEST_LITELLM_PROXY_BASE_URL");
}

#[test]
fn uses_litellm_proxy_base_url_env_fallback() {
    std::env::set_var("LITELLM_PROXY_BASE_URL", "https://fallback.example.com");
    let file = write_config(
        r#"
general_settings:
  database_url: postgres://test
"#,
    );

    let config = load_config(file.path()).unwrap();
    assert_eq!(
        config.mcp_servers.proxy_base_url(),
        Some("https://fallback.example.com")
    );
    std::env::remove_var("LITELLM_PROXY_BASE_URL");
}

#[test]
fn accepts_authentication_token_alias() {
    let file = write_config(
        r#"
mcp_servers:
  s:
    url: https://mcp.example.com/mcp
    auth_type: bearer_token
    authentication_token: sk-alias
"#,
    );
    let config = load_config(file.path()).unwrap();
    assert_eq!(
        config.mcp_servers["s"].auth_value.as_deref(),
        Some("sk-alias")
    );
}

#[test]
fn rejects_old_list_format_with_migration_hint() {
    let file = write_config(
        r#"
mcp_servers:
  - id: linear
    url: https://mcp.linear.app/mcp
"#,
    );
    let err = load_config(file.path()).unwrap_err().to_string();
    assert!(err.contains("dict keyed by server name"), "got: {err}");
}

#[test]
fn rejects_unsupported_transport() {
    for transport in ["sse", "stdio"] {
        let file = write_config(&format!(
            "mcp_servers:\n  s:\n    url: https://mcp.example.com/mcp\n    transport: {transport}\n"
        ));
        assert!(
            load_config(file.path()).is_err(),
            "{transport} should reject"
        );
    }
}

#[test]
fn rejects_unsupported_auth_types() {
    for auth in ["oauth2", "oauth2_token_exchange", "aws_sigv4"] {
        let file = write_config(&format!(
            "mcp_servers:\n  s:\n    url: https://mcp.example.com/mcp\n    auth_type: {auth}\n    auth_value: x\n"
        ));
        assert!(load_config(file.path()).is_err(), "{auth} should reject");
    }
}

#[test]
fn rejects_auth_type_without_value() {
    let file = write_config(
        r#"
mcp_servers:
  s:
    url: https://mcp.example.com/mcp
    auth_type: bearer_token
"#,
    );
    assert!(load_config(file.path()).is_err());
}

/// Golden compatibility test: the published LiteLLM docs `mcp_servers` HTTP
/// example parses. If LiteLLM changes their schema, this breaks and tells us.
#[test]
fn parses_litellm_docs_http_example() {
    let file = write_config(
        r#"
mcp_servers:
  deepwiki_mcp:
    url: "https://mcp.deepwiki.com/mcp"
  my_http_server:
    url: "https://my-mcp-server.com/mcp"
    transport: "http"
    description: "My custom MCP server"
    auth_type: "api_key"
    auth_value: "abc123"
"#,
    );
    let config = load_config(file.path()).unwrap();
    assert_eq!(config.mcp_servers.len(), 2);
    assert_eq!(
        config.mcp_servers["my_http_server"].auth_type,
        McpAuthType::ApiKey
    );
    assert_eq!(
        config.mcp_servers["my_http_server"].description.as_deref(),
        Some("My custom MCP server")
    );
}

/// The CircleCI stdio example from LiteLLM docs — not yet supported.
#[test]
fn rejects_litellm_docs_stdio_example() {
    let file = write_config(
        r#"
mcp_servers:
  circleci_mcp:
    transport: "stdio"
    url: ""
    command: "npx"
    args: ["-y", "@circleci/mcp-server-circleci"]
"#,
    );
    assert!(load_config(file.path()).is_err());
}

#[test]
fn expands_database_url_from_general_settings() {
    std::env::set_var(
        "TEST_LITELLM_DATABASE_URL",
        "postgres:///litellm_rust_managed_agents_test",
    );
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"
general_settings:
  database_url: os.environ/TEST_LITELLM_DATABASE_URL
"#
    )
    .unwrap();

    let config = load_config(file.path()).unwrap();
    assert_eq!(
        config.general_settings.database_url.as_deref(),
        Some("postgres:///litellm_rust_managed_agents_test")
    );
}

#[test]
fn rejects_public_base_url_without_http_scheme() {
    let file = write_config(
        r#"
general_settings:
  database_url: postgres://test
  public_base_url: localhost:4000
"#,
    );

    let err = load_config(file.path()).unwrap_err().to_string();
    assert!(
        err.contains("general_settings.public_base_url must be an absolute http(s) URL"),
        "got: {err}"
    );
}

#[test]
fn rejects_proxy_base_url_without_http_scheme() {
    let file = write_config(
        r#"
general_settings:
  database_url: postgres://test
mcp_servers:
  proxy_base_url: localhost:4000
"#,
    );

    let err = load_config(file.path()).unwrap_err().to_string();
    assert!(
        err.contains("mcp_servers.proxy_base_url must be an absolute http(s) URL"),
        "got: {err}"
    );
}

#[test]
fn loads_config_defined_agent_and_expands_e2b_key() {
    std::env::set_var("E2B_API_KEY", "e2b-test");
    std::env::set_var("ANTHROPIC_API_KEY", "anthropic-test");

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"
general_settings:
  sandbox_choice: e2b
  e2b_sandbox_params:
    e2b_api_key: os.environ/E2B_API_KEY
    e2b_template: litellm-4gb
    envs:
      ANTHROPIC_API_KEY: os.environ/ANTHROPIC_API_KEY
agents:
  - name: Untitled agent
    description: A blank starting point with the core toolset.
    model: claude-sonnet-4-6
    system: You are a general-purpose agent that can research, write code, run commands, and use connected tools to complete the user's task end to end.
    mcp_servers: []
    tools:
      - type: agent_toolset_20260401
    skills: []
"#
    )
    .unwrap();

    let config = load_config(file.path()).unwrap();
    assert!(config.model_list.is_empty());
    assert_eq!(config.agents[0].id(), "untitled-agent");
    assert_eq!(config.agents[0].model, "claude-sonnet-4-6");
    assert_eq!(
        config
            .general_settings
            .e2b_sandbox_params
            .e2b_api_key
            .as_deref(),
        Some("e2b-test")
    );
    assert_eq!(
        config.general_settings.e2b_sandbox_params.e2b_template,
        "litellm-4gb"
    );
    assert_eq!(
        config
            .general_settings
            .e2b_sandbox_params
            .envs
            .get("ANTHROPIC_API_KEY")
            .map(String::as_str),
        Some("anthropic-test")
    );
}
