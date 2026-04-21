use std::path::{Path, PathBuf};

use crate::config::ThreadConfigBuilder;
use crate::thread::Thread;
use crate::thread_trace_store::{ThreadTraceKind, ThreadTraceMetadata, recover_thread_metadata};
use crate::turn_log_store::recover_thread_log_state;
use crate::{ThreadConfig, TraceConfig, TurnConfig};
use argus_protocol::ThreadId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThreadBootstrapError {
    #[error("thread bootstrap failed: {0}")]
    Failed(String),
}

pub async fn recover_and_validate_metadata(
    base_dir: &Path,
    expected_thread_id: ThreadId,
    expected_kind: ThreadTraceKind,
) -> Result<ThreadTraceMetadata, ThreadBootstrapError> {
    let metadata = recover_thread_metadata(base_dir)
        .await
        .map_err(|err| ThreadBootstrapError::Failed(err.to_string()))?;
    if metadata.thread_id != expected_thread_id {
        return Err(ThreadBootstrapError::Failed(format!(
            "{expected_kind:?} trace metadata for {expected_thread_id} resolved to {}",
            metadata.thread_id
        )));
    }
    if metadata.kind != expected_kind {
        return Err(ThreadBootstrapError::Failed(format!(
            "thread {expected_thread_id} is not recorded as {expected_kind:?}"
        )));
    }
    Ok(metadata)
}

pub fn build_thread_config(
    base_dir: PathBuf,
    model_name: String,
) -> Result<ThreadConfig, ThreadBootstrapError> {
    let trace_cfg = TraceConfig::new(true, base_dir).with_model(Some(model_name));
    let mut turn_config = TurnConfig::new();
    turn_config.trace_config = Some(trace_cfg);
    ThreadConfigBuilder::default()
        .turn_config(turn_config)
        .build()
        .map_err(|err| ThreadBootstrapError::Failed(err.to_string()))
}

pub async fn hydrate_turn_log_state(
    thread: &mut Thread,
    base_dir: &Path,
    updated_at: &str,
) -> Result<(), ThreadBootstrapError> {
    let updated_at = chrono::DateTime::parse_from_rfc3339(updated_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let recovered = recover_thread_log_state(base_dir)
        .await
        .map_err(|err| ThreadBootstrapError::Failed(err.to_string()))?;
    if recovered.turn_count() > 0 {
        thread.hydrate_from_turn_log_state(recovered, updated_at);
    }
    Ok(())
}

pub async fn cleanup_trace_dir(base_dir: &Path) {
    match tokio::fs::remove_dir_all(base_dir).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(
                path = %base_dir.display(),
                error = %error,
                "failed to clean up thread trace directory"
            );
        }
    }
}
