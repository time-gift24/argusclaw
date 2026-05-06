use std::path::PathBuf;

use argus_protocol::{ArgusError, Result};

pub(crate) enum DatabaseTarget {
    PostgresUrl(String),
}

pub(crate) fn resolve_database_target(configured: Option<&str>) -> Result<DatabaseTarget> {
    let configured = configured
        .map(str::to_string)
        .unwrap_or_else(default_database_target);

    if configured.starts_with("postgres://") || configured.starts_with("postgresql://") {
        return Ok(DatabaseTarget::PostgresUrl(configured));
    }

    Err(ArgusError::DatabaseError {
        reason: "argus-server requires a PostgreSQL DATABASE_URL (postgres:// or postgresql://)"
            .to_string(),
    })
}

pub(crate) fn default_trace_dir() -> PathBuf {
    std::env::var("TRACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".arguswing")
                .join("traces")
        })
}

fn default_database_target() -> String {
    std::env::var("DATABASE_URL").unwrap_or_default()
}
