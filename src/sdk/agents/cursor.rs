//! Cursor-specific helpers re-exported from the cursor runtime provider.
//!
//! This shim exists so that `session_events.rs` (which uses `super::cursor::*`)
//! can access cursor utilities without knowing the provider module layout.

pub(super) use crate::sdk::providers::cursor::runtime::{
    agent_id_from_context, run_id,
};
