use std::path::{Path, PathBuf};

use tokio::fs;

use argus_protocol::{AgentRecord, SessionId, ThreadId};
use serde::{Deserialize, Serialize};

use crate::error::TurnLogError;

const THREAD_METADATA_FILE: &str = "thread.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadTraceKind {
    ChatRoot,
    Job,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadTraceMetadata {
    pub thread_id: ThreadId,
    pub kind: ThreadTraceKind,
    pub root_session_id: Option<SessionId>,
    pub parent_thread_id: Option<ThreadId>,
    pub child_thread_ids: Vec<ThreadId>,
    pub agent_snapshot: AgentRecord,
}

#[must_use]
pub fn chat_thread_base_dir(
    trace_root: &Path,
    session_id: SessionId,
    thread_id: ThreadId,
) -> PathBuf {
    trace_root
        .join(session_id.to_string())
        .join(thread_id.to_string())
}

#[must_use]
pub fn child_thread_base_dir(parent_base_dir: &Path, thread_id: ThreadId) -> PathBuf {
    parent_base_dir.join(thread_id.to_string())
}

#[must_use]
pub fn thread_metadata_path(base_dir: &Path) -> PathBuf {
    base_dir.join(THREAD_METADATA_FILE)
}

pub async fn persist_thread_metadata(
    base_dir: &Path,
    metadata: &ThreadTraceMetadata,
) -> Result<(), TurnLogError> {
    fs::create_dir_all(base_dir)
        .await
        .map_err(|error| TurnLogError::ThreadMetadataIo {
            path: base_dir.to_path_buf(),
            reason: format!("failed to create thread trace dir: {error}"),
        })?;

    let contents = serde_json::to_vec_pretty(metadata).map_err(|error| {
        TurnLogError::ThreadMetadataMalformed {
            path: thread_metadata_path(base_dir),
            reason: error.to_string(),
        }
    })?;

    fs::write(thread_metadata_path(base_dir), contents)
        .await
        .map_err(|error| TurnLogError::ThreadMetadataIo {
            path: thread_metadata_path(base_dir),
            reason: format!("failed to write thread metadata: {error}"),
        })?;

    Ok(())
}

pub async fn recover_thread_metadata(base_dir: &Path) -> Result<ThreadTraceMetadata, TurnLogError> {
    let path = thread_metadata_path(base_dir);
    let content = fs::read_to_string(&path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            TurnLogError::ThreadMetadataNotFound(path.clone())
        } else {
            TurnLogError::ThreadMetadataIo {
                path: path.clone(),
                reason: format!("failed to read thread metadata: {error}"),
            }
        }
    })?;

    serde_json::from_str(&content).map_err(|error| TurnLogError::ThreadMetadataMalformed {
        path,
        reason: error.to_string(),
    })
}

pub async fn append_child_thread_id(
    parent_base_dir: &Path,
    child_thread_id: ThreadId,
) -> Result<ThreadTraceMetadata, TurnLogError> {
    let mut metadata = recover_thread_metadata(parent_base_dir).await?;
    if !metadata.child_thread_ids.contains(&child_thread_id) {
        metadata.child_thread_ids.push(child_thread_id);
        persist_thread_metadata(parent_base_dir, &metadata).await?;
    }
    Ok(metadata)
}

pub async fn find_job_thread_base_dir(
    trace_root: &Path,
    thread_id: ThreadId,
) -> Result<PathBuf, TurnLogError> {
    let mut pending_dirs = vec![trace_root.to_path_buf()];
    let mut matches = Vec::new();

    while let Some(dir) = pending_dirs.pop() {
        let mut entries = match fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(TurnLogError::ThreadMetadataIo {
                    path: dir.clone(),
                    reason: format!("failed to scan thread trace tree: {error}"),
                });
            }
        };

        while let Some(entry) =
            entries
                .next_entry()
                .await
                .map_err(|error| TurnLogError::ThreadMetadataIo {
                    path: dir.clone(),
                    reason: format!("failed to enumerate thread trace tree: {error}"),
                })?
        {
            let path = entry.path();
            let file_type =
                entry
                    .file_type()
                    .await
                    .map_err(|error| TurnLogError::ThreadMetadataIo {
                        path: path.clone(),
                        reason: format!("failed to inspect trace entry type: {error}"),
                    })?;
            if file_type.is_dir() {
                pending_dirs.push(path);
                continue;
            }

            if path.file_name().and_then(|name| name.to_str()) != Some(THREAD_METADATA_FILE) {
                continue;
            }

            let base_dir = path.parent().map(Path::to_path_buf).ok_or_else(|| {
                TurnLogError::ThreadMetadataMalformed {
                    path: path.clone(),
                    reason: "thread metadata path has no parent directory".to_string(),
                }
            })?;
            let metadata = recover_thread_metadata(&base_dir).await?;
            if metadata.thread_id != thread_id {
                continue;
            }
            if metadata.kind != ThreadTraceKind::Job {
                return Err(TurnLogError::ThreadMetadataMalformed {
                    path: thread_metadata_path(&base_dir),
                    reason: format!("thread {thread_id} is not recorded as a job thread"),
                });
            }
            matches.push(base_dir);
        }
    }

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(TurnLogError::ThreadMetadataNotFound(
            trace_root
                .join(thread_id.to_string())
                .join(THREAD_METADATA_FILE),
        )),
        _ => Err(TurnLogError::ThreadMetadataMalformed {
            path: trace_root.to_path_buf(),
            reason: format!("multiple job trace directories found for thread {thread_id}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use argus_protocol::AgentRecord;

    fn sample_metadata(thread_id: ThreadId) -> ThreadTraceMetadata {
        ThreadTraceMetadata {
            thread_id,
            kind: ThreadTraceKind::ChatRoot,
            root_session_id: Some(SessionId::new()),
            parent_thread_id: None,
            child_thread_ids: Vec::new(),
            agent_snapshot: AgentRecord {
                system_prompt: "You are a snapshot test agent.".to_string(),
                display_name: "Snapshot Agent".to_string(),
                ..AgentRecord::default()
            },
        }
    }

    #[tokio::test]
    async fn persist_and_recover_thread_metadata_roundtrip() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path().join("thread");
        let thread_id = ThreadId::new();
        let metadata = sample_metadata(thread_id);

        persist_thread_metadata(&base_dir, &metadata)
            .await
            .expect("metadata should persist");

        let recovered = recover_thread_metadata(&base_dir)
            .await
            .expect("metadata should recover");

        assert_eq!(recovered.thread_id, thread_id);
        assert_eq!(recovered.kind, ThreadTraceKind::ChatRoot);
        assert_eq!(
            recovered.agent_snapshot.system_prompt,
            "You are a snapshot test agent."
        );
        assert_eq!(recovered.agent_snapshot.display_name, "Snapshot Agent");
    }

    #[tokio::test]
    async fn append_child_thread_id_deduplicates() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let parent_base_dir = temp_dir.path().join("parent");
        let parent_id = ThreadId::new();
        let child_id = ThreadId::new();
        let metadata = sample_metadata(parent_id);

        persist_thread_metadata(&parent_base_dir, &metadata)
            .await
            .expect("metadata should persist");

        append_child_thread_id(&parent_base_dir, child_id)
            .await
            .expect("child should be appended");
        append_child_thread_id(&parent_base_dir, child_id)
            .await
            .expect("child append should deduplicate");

        let recovered = recover_thread_metadata(&parent_base_dir)
            .await
            .expect("metadata should recover");
        assert_eq!(recovered.child_thread_ids, vec![child_id]);
    }

    #[tokio::test]
    async fn find_job_thread_base_dir_locates_nested_job() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let root_id = ThreadId::new();
        let child_id = ThreadId::new();
        let root_base_dir = chat_thread_base_dir(temp_dir.path(), session_id, root_id);
        let child_base_dir = child_thread_base_dir(&root_base_dir, child_id);

        persist_thread_metadata(
            &root_base_dir,
            &ThreadTraceMetadata {
                thread_id: root_id,
                kind: ThreadTraceKind::ChatRoot,
                root_session_id: Some(session_id),
                parent_thread_id: None,
                child_thread_ids: vec![child_id],
                agent_snapshot: AgentRecord::default(),
            },
        )
        .await
        .expect("root metadata should persist");
        persist_thread_metadata(
            &child_base_dir,
            &ThreadTraceMetadata {
                thread_id: child_id,
                kind: ThreadTraceKind::Job,
                root_session_id: Some(session_id),
                parent_thread_id: Some(root_id),
                child_thread_ids: Vec::new(),
                agent_snapshot: AgentRecord::default(),
            },
        )
        .await
        .expect("child metadata should persist");

        let found = find_job_thread_base_dir(temp_dir.path(), child_id)
            .await
            .expect("job base dir should be found");
        assert_eq!(found, child_base_dir);
    }
}
