use std::path::PathBuf;

use argus_protocol::{ArgusError, Result};

pub(crate) enum DatabaseTarget {
    Url(String),
    Path(PathBuf),
}

pub(crate) fn resolve_database_target(configured: Option<&str>) -> Result<DatabaseTarget> {
    let configured = configured
        .map(str::to_string)
        .unwrap_or_else(default_database_target);

    if configured.starts_with("sqlite:") {
        return Ok(DatabaseTarget::Url(configured));
    }

    Ok(DatabaseTarget::Path(expand_home_path(&configured)?))
}

pub(crate) fn ensure_parent_dir(path: &std::path::Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| ArgusError::DatabaseError {
        reason: format!("Invalid database path: {}", path.display()),
    })?;
    std::fs::create_dir_all(parent).map_err(|error| ArgusError::DatabaseError {
        reason: format!("Cannot create database directory: {error}"),
    })?;
    Ok(())
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
    std::env::var("DATABASE_URL").unwrap_or_else(|_| "~/.arguswing/sqlite.db".to_string())
}

fn expand_home_path(path: &str) -> Result<PathBuf> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let home_dir = dirs::home_dir().ok_or_else(|| ArgusError::DatabaseError {
            reason: "Cannot determine home directory".to_string(),
        })?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(PathBuf::from(path))
}
