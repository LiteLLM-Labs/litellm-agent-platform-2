use std::sync::Arc;

use axum::{
    routing::{any, delete, get, post, put},
    Router,
};

use crate::{
    http::{
        agents::events,
        capabilities::capabilities,
        health::health,
        messages::messages,
        models::models,
        openapi::{openapi_json, swagger_ui},
        responses::responses,
        sessions, ui,
    },
    mcp::route::{streamable_http, streamable_http_server},
    proxy::state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    public_routes()
        .merge(api_routes())
        .merge(session_routes())
        .merge(crate::http::observability::routes::router())
        .merge(crate::http::management::routes::router())
        .merge(crate::http::managed_agents::routes::router())
        .merge(mcp_routes())
        .fallback_service(ui::static_files())
        .with_state(state)
}

fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(ui::redirect_to_sessions))
        .route("/docs", get(swagger_ui))
        .route("/openapi.json", get(openapi_json))
        .route("/health", get(health))
        .route("/event", get(events))
        .route("/v1/messages", post(messages))
        .route("/v1/responses", post(responses))
        .route("/v1/models", get(models))
        .route(
            "/v1/sessions/{session_id}/events/stream",
            get(sessions::runtime_events),
        )
        .route(
            "/v1/sessions/{session_id}/events",
            get(sessions::runtime_event_list),
        )
}

fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/harness-proxy/{*path}",
            any(crate::http::harness_proxy::proxy),
        )
        .route("/api/capabilities", get(capabilities))
        .route("/api/platform-mcps", get(crate::http::platform_mcps::list))
        .route(
            "/api/agent-runtimes",
            get(crate::http::agent_runtimes::list),
        )
        .route(
            "/api/agent-runtimes/{runtime}/credentials",
            put(crate::http::agent_runtimes::save).delete(crate::http::agent_runtimes::delete),
        )
        .route(
            "/api/providers",
            get(crate::http::provider_credentials::list),
        )
        .route(
            "/api/providers/{provider_id}",
            post(crate::http::provider_credentials::save_provider)
                .delete(crate::http::provider_credentials::delete_provider),
        )
        .route(
            "/api/vault/{user_id}",
            get(crate::http::vault::list).post(crate::http::vault::save),
        )
        .route(
            "/api/vault/{user_id}/{key}",
            delete(crate::http::vault::delete),
        )
}

fn mcp_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/mcp",
            get(streamable_http)
                .post(streamable_http)
                .delete(streamable_http),
        )
        .route(
            "/mcp/{server_id}",
            get(streamable_http_server)
                .post(streamable_http_server)
                .delete(streamable_http_server),
        )
        .route(
            "/mcp/platform/{agent_id}",
            post(crate::http::platform_mcps::serve),
        )
}

fn session_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/session", get(sessions::list).post(sessions::create))
        .route(
            "/session/{session_id}",
            get(sessions::get).delete(sessions::delete),
        )
        .route(
            "/session/{session_id}/message",
            get(sessions::messages).post(sessions::send_message),
        )
        .route(
            "/session/{session_id}/prompt_async",
            post(sessions::prompt_async),
        )
        .route(
            "/session/{session_id}/runtime_events",
            get(sessions::runtime_events),
        )
        .route(
            "/session/{session_id}/runtime_events/list",
            get(sessions::runtime_event_list),
        )
        .route("/session/{session_id}/abort", post(sessions::abort))
}
