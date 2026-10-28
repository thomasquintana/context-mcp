//! Multimodal context and semantic memory for HTLK-orchestrated agents.
//!
//! This facade exposes domain contracts and the authenticated server foundation.
//! See the repository specification for implemented operations and pending features.

pub use context_core as core;
pub mod config;
pub mod mcp;
pub mod transport;
