//! Provider-independent contracts for a multi-tenant context engine.
//!
//! Authentication and authoritative records must be established by the host before
//! evaluating these contracts. Deserialized claims are never authorization grants.

pub mod access;
pub mod content;
pub mod context;
pub mod ids;
pub mod jobs;
pub mod memory;
pub mod purge;
