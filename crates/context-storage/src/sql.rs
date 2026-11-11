//! PostgreSQL-only SQLx query interface shared by trusted application services.
//!
//! Exact-version core and PostgreSQL dependencies keep unused native SQLite drivers
//! out of resolution. These are upstream SQLx types/functions with no new execution
//! or transaction behavior; SQLx remains responsible for connections and migrations.

pub use sqlx_core::{
    error::Error, migrate, query::query, query_scalar::query_scalar, row::Row,
    transaction::Transaction,
};
pub use sqlx_postgres::{self as postgres, PgPool, Postgres};
