//! Multimodal context and semantic memory for HTLK-orchestrated agents.
//!
//! This facade exposes domain contracts, authenticated MCP serving and the HTLK CLI.
//! See the repository specification and permanent verification evidence for boundaries.

pub use context_core as core;
pub mod client;
pub mod config;
pub mod mcp;
pub mod transport;
