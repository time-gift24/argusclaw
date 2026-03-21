//! Database utilities for ArgusWing.

use std::path::PathBuf;

use argus_protocol::{ArgusError, Result};

pub(crate) enum DatabaseTarget {
    Url(String),
    Path(std::path::PathBuf),
}

pub(crate) fn resolve_database_target(configured: Option<&str>) -> Result<DatabaseTarget> {
    let configured = configured
        .map(|s| s.to_string())
        .unwrap_or_else(default_database_target);

    if configured.starts_with("sqlite:") {
        return Ok(DatabaseTarget::Url(configured));
    }

    Ok(DatabaseTarget::Path(expand_home_path(&configured)?))
}

fn default_database_target() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| "~/.arguswing/sqlite.db".to_string())
}

fn expand_home_path(path: &str) -> Result<std::path::PathBuf> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let home_dir = dirs::home_dir().ok_or_else(|| ArgusError::DatabaseError {
            reason: "Cannot determine home directory".to_string(),
        })?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(std::path::PathBuf::from(path))
}

pub(crate) fn ensure_parent_dir(path: &std::path::Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| ArgusError::DatabaseError {
        reason: format!("Invalid database path: {}", path.display()),
    })?;
    std::fs::create_dir_all(parent).map_err(|e| ArgusError::DatabaseError {
        reason: format!("Cannot create database directory: {}", e),
    })?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn database_path_to_string(target: &DatabaseTarget) -> String {
    match target {
        DatabaseTarget::Url(url) => url.clone(),
        DatabaseTarget::Path(path) => path.display().to_string(),
    }
}

/// Returns the default directory for turn trace files.
///
/// Order of precedence:
/// 1. `TRACE_DIR` environment variable
/// 2. `~/.arguswing/traces/`
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
