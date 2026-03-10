mod agent;
mod llm;

use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::db::DbError;

pub use agent::SqliteAgentRepository;
pub use llm::SqliteLlmProviderRepository;

pub async fn connect(database: &str) -> Result<SqlitePool, DbError> {
    let options = if database.starts_with("sqlite:") {
        SqliteConnectOptions::from_str(database).map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })?
    } else {
        SqliteConnectOptions::new().filename(database)
    };

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options.create_if_missing(true))
        .await
        .map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })
}

pub async fn connect_path(path: &Path) -> Result<SqlitePool, DbError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::migrate!()
        .run(pool)
        .await
        .map_err(|e| DbError::MigrationFailed {
            reason: e.to_string(),
        })
}
