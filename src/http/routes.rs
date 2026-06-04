use std::sync::Arc;

use axum::{
    routing::{get, post},
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
    Router::new()
        .route("/", get(ui::redirect_to_sessions))
        .route("/docs", get(swagger_ui))
        .route("/openapi.json", get(openapi_json))
        .route("/health", get(health))
        .route("/event", get(events))
        .route("/api/capabilities", get(capabilities))
        .route(
            "/api/providers",
            get(crate::http::provider_credentials::list),
        )
        .route(
            "/api/providers/{provider_id}",
            post(crate::http::provider_credentials::save_provider)
                .delete(crate::http::provider_credentials::delete_provider),
        )
        .route("/v1/messages", post(messages))
        .route("/v1/responses", post(responses))
        .route("/v1/models", get(models))
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
        .route("/session/{session_id}/abort", post(sessions::abort))
        .merge(crate::http::management::routes::router())
        .merge(crate::http::managed_agents::routes::router())
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
        .fallback_service(ui::static_files())
        .with_state(state)
}
