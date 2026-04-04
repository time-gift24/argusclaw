use std::path::{Path, PathBuf};

use tokio::fs;

use argus_protocol::{AgentRecord, SessionId, ThreadId};

use crate::error::TurnLogError;

const THREAD_SNAPSHOT_FILE: &str = "thread.json";

#[must_use]
pub fn thread_base_dir(
    trace_dir: &Path,
    session_id: Option<SessionId>,
    thread_id: ThreadId,
) -> PathBuf {
    match session_id {
        Some(session_id) => trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string()),
        None => trace_dir.join(thread_id.to_string()),
    }
}

#[must_use]
pub fn thread_snapshot_path(base_dir: &Path) -> PathBuf {
    base_dir.join(THREAD_SNAPSHOT_FILE)
}

pub async fn persist_thread_snapshot(
    base_dir: &Path,
    agent_record: &AgentRecord,
) -> Result<(), TurnLogError> {
    fs::create_dir_all(base_dir)
        .await
        .map_err(|error| TurnLogError::ThreadSnapshotIo {
            path: base_dir.to_path_buf(),
            reason: format!("failed to create thread trace dir: {error}"),
        })?;

    let contents = serde_json::to_vec_pretty(agent_record).map_err(|error| {
        TurnLogError::ThreadSnapshotMalformed {
            path: thread_snapshot_path(base_dir),
            reason: error.to_string(),
        }
    })?;

    fs::write(thread_snapshot_path(base_dir), contents)
        .await
        .map_err(|error| TurnLogError::ThreadSnapshotIo {
            path: thread_snapshot_path(base_dir),
            reason: format!("failed to write thread snapshot: {error}"),
        })?;

    Ok(())
}

pub async fn recover_thread_snapshot(base_dir: &Path) -> Result<AgentRecord, TurnLogError> {
    let path = thread_snapshot_path(base_dir);
    let content = fs::read_to_string(&path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            TurnLogError::ThreadSnapshotNotFound(path.clone())
        } else {
            TurnLogError::ThreadSnapshotIo {
                path: path.clone(),
                reason: format!("failed to read thread snapshot: {error}"),
            }
        }
    })?;

    serde_json::from_str(&content).map_err(|error| TurnLogError::ThreadSnapshotMalformed {
        path,
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use argus_protocol::AgentRecord;

    #[tokio::test]
    async fn persist_and_recover_thread_snapshot_roundtrip() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path().join("thread");
        let agent_record = AgentRecord {
            system_prompt: "You are a snapshot test agent.".to_string(),
            display_name: "Snapshot Agent".to_string(),
            ..AgentRecord::default()
        };

        persist_thread_snapshot(&base_dir, &agent_record)
            .await
            .expect("snapshot should persist");

        let recovered = recover_thread_snapshot(&base_dir)
            .await
            .expect("snapshot should recover");

        assert_eq!(recovered.system_prompt, "You are a snapshot test agent.");
        assert_eq!(recovered.display_name, "Snapshot Agent");
    }
}
