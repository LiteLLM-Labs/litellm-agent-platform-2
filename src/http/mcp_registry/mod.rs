pub mod admin;
pub mod proxy;
pub mod public;
pub mod user_credentials;

use axum::http::HeaderMap;

use crate::proxy::{auth::master_key::require_master_key, state::AppState};

/// Return the caller's user id for vault key operations.
///
/// Only callers presenting the explicitly configured master key may supply
/// `x-user-id` for impersonation. When no master key is configured, or the
/// caller uses a regular gateway key, user identity is always `"default"`.
pub(super) fn caller_user_id(headers: &HeaderMap, state: &AppState) -> String {
    let Some(master_key) = state.config.general_settings.master_key.as_deref() else {
        return "default".to_owned();
    };
    let is_admin = require_master_key(headers, Some(master_key)).is_ok();
    if is_admin {
        headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "default".to_owned())
    } else {
        "default".to_owned()
    }
}
