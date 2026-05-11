use argus_protocol::{ArgusError, Result};

pub(crate) enum DatabaseTarget {
    PostgresUrl(String),
}

pub(crate) fn resolve_database_target(configured: Option<&str>) -> Result<DatabaseTarget> {
    let configured = configured.map(str::to_string).unwrap_or_default();

    if configured.starts_with("postgres://") || configured.starts_with("postgresql://") {
        return Ok(DatabaseTarget::PostgresUrl(configured));
    }

    Err(ArgusError::DatabaseError {
        reason: "argus-server requires a PostgreSQL [database].url (postgres:// or postgresql://)"
            .to_string(),
    })
}
