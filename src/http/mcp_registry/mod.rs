pub mod admin;
pub mod proxy;
pub mod public;
pub mod user_credentials;

use axum::http::HeaderMap;

use crate::proxy::state::AppState;

/// Return the caller's user id for vault key operations.
///
/// Any authenticated caller may supply `x-user-id` to scope their personal
/// credentials. Falls back to `"default"` when the header is absent.
pub(super) fn caller_user_id(headers: &HeaderMap, _state: &AppState) -> String {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "default".to_owned())
}
