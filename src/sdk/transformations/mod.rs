//! Shared SDK transformation contracts.
//!
//! Provider endpoint modules and runtime adapters implement these traits, while
//! routing and provider folders stay focused on lookup and ownership.

pub mod anthropic_messages;
pub mod base;
pub mod openai_responses;
pub mod runtime;
