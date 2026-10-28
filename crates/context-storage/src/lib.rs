//! Authoritative application persistence, independent of provider execution.
//!
//! Repositories are trusted infrastructure. Application services authenticate and
//! authorize the caller before mutations; database constraints independently fence
//! ancestry, revision publication and idempotent state changes.

pub mod postgres;

use std::{error::Error as StdError, fmt};

/// Redacted storage boundary failures. Backend details are available only as causes.
#[derive(thiserror::Error)]
pub enum Error {
    /// Database operation failed.
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    /// Migration failed.
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    /// Invalid authoritative data or a rejected parameter.
    #[error("invalid storage contract")]
    Invalid,
    /// Missing resource, without disclosing forbidden resource existence.
    #[error("resource is unavailable")]
    Unavailable,
    /// Stale revision, reused key or conflicting write.
    #[error("conflicting storage operation")]
    Conflict,
    /// Serialization of an authoritative domain record failed.
    #[error("invalid stored record")]
    Serialization(#[from] serde_json::Error),
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display intentionally omits connection strings, SQL, values and causes.
        write!(f, "StorageError({self})")
    }
}

/// Result of an authoritative storage operation.
pub type Result<T> = std::result::Result<T, Error>;

/// Whether an error is a PostgreSQL constraint violation, for trusted diagnostics.
pub fn is_constraint_violation(error: &Error) -> bool {
    error
        .source()
        .and_then(|cause| cause.downcast_ref::<sqlx::Error>())
        .and_then(|error| error.as_database_error())
        .and_then(|error| error.code())
        .is_some_and(|code| code.starts_with("23"))
}
