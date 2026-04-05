use std::path::PathBuf;
use std::sync::Arc;

use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;
use argus_tool::{KnowledgeTool, ToolManager};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio::sync::broadcast;

#[derive(Debug, Parser)]
#[command(
    name = "argus-knowledge-cli",
    version,
    about = "Manual smoke test for Knowledge tool"
)]
struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true, default_value_t = false)]
    pretty: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List all registered knowledge repos
    ListRepos,
    /// Resolve a snapshot for a repo
    ResolveSnapshot {
        /// Repository identifier
        #[arg(long)]
        repo_id: String,
        /// Git reference (defaults to repo's default branch)
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Explore the file tree of a snapshot
    ExploreTree {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Snapshot identifier (overrides repo_id)
        #[arg(long = "snapshot-id")]
        snapshot_id: Option<String>,
        /// Git reference (used with repo_id to resolve snapshot)
        #[arg(long)]
        r#ref: Option<String>,
        /// Path to explore (defaults to root)
        #[arg(long, default_value = "/")]
        path: String,
        /// Exploration depth (defaults to 1)
        #[arg(long, default_value_t = 1)]
        depth: usize,
    },
    /// Search nodes by keyword
    SearchNodes {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Snapshot identifier (overrides repo_id)
        #[arg(long = "snapshot-id")]
        snapshot_id: Option<String>,
        /// Git reference (used with repo_id to resolve snapshot)
        #[arg(long)]
        r#ref: Option<String>,
        /// Search query
        #[arg(long)]
        query: String,
        /// Scope path for search
        #[arg(long)]
        scope_path: Option<String>,
        /// Maximum results (defaults to 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Get a specific node by ID
    GetNode {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Snapshot identifier (overrides repo_id)
        #[arg(long = "snapshot-id")]
        snapshot_id: Option<String>,
        /// Git reference (used with repo_id to resolve snapshot)
        #[arg(long)]
        r#ref: Option<String>,
        /// Node identifier
        #[arg(long = "node-id")]
        node_id: String,
    },
    /// Read node content with pagination
    GetContent {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Snapshot identifier (overrides repo_id)
        #[arg(long = "snapshot-id")]
        snapshot_id: Option<String>,
        /// Git reference (used with repo_id to resolve snapshot)
        #[arg(long)]
        r#ref: Option<String>,
        /// Node identifier
        #[arg(long = "node-id")]
        node_id: String,
        /// Maximum characters to return
        #[arg(long)]
        max_chars: Option<usize>,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Get neighbors of a node
    GetNeighbors {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Snapshot identifier (overrides repo_id)
        #[arg(long = "snapshot-id")]
        snapshot_id: Option<String>,
        /// Git reference (used with repo_id to resolve snapshot)
        #[arg(long)]
        r#ref: Option<String>,
        /// Node identifier
        #[arg(long = "node-id")]
        node_id: String,
    },
    /// Create or update a knowledge PR from a JSON payload file
    CreateKnowledgePr {
        /// Path to a JSON file matching the knowledge tool request shape
        #[arg(long = "payload-file")]
        payload_file: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = run(cli.command).await;

    let output = if cli.pretty {
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
            String::from("{\"ok\":false,\"error\":\"failed to format output\"}")
        })
    } else {
        serde_json::to_string(&result).unwrap_or_else(|_| {
            String::from("{\"ok\":false,\"error\":\"failed to format output\"}")
        })
    };

    println!("{output}");
}

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (pipe_tx, _) = broadcast::channel(16);
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
    })
}

async fn payload_for_command(
    command: &Command,
) -> Result<(&'static str, serde_json::Value), String> {
    match command {
        Command::ListRepos => Ok(("list_repos", json!({ "action": "list_repos" }))),
        Command::ResolveSnapshot { repo_id, r#ref } => Ok((
            "resolve_snapshot",
            json!({
                "action": "resolve_snapshot",
                "repo_id": repo_id,
                "ref": r#ref,
            }),
        )),
        Command::ExploreTree {
            repo_id,
            snapshot_id,
            r#ref,
            path,
            depth,
        } => Ok((
            "explore_tree",
            json!({
                "action": "explore_tree",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "path": path,
                "depth": depth,
            }),
        )),
        Command::SearchNodes {
            repo_id,
            snapshot_id,
            r#ref,
            query,
            scope_path,
            limit,
        } => Ok((
            "search_nodes",
            json!({
                "action": "search_nodes",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "query": query,
                "scope_path": scope_path,
                "limit": limit,
            }),
        )),
        Command::GetNode {
            repo_id,
            snapshot_id,
            r#ref,
            node_id,
        } => Ok((
            "get_node",
            json!({
                "action": "get_node",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "node_id": node_id,
            }),
        )),
        Command::GetContent {
            repo_id,
            snapshot_id,
            r#ref,
            node_id,
            max_chars,
            cursor,
        } => Ok((
            "get_content",
            json!({
                "action": "get_content",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "node_id": node_id,
                "max_chars": max_chars,
                "cursor": cursor,
            }),
        )),
        Command::GetNeighbors {
            repo_id,
            snapshot_id,
            r#ref,
            node_id,
        } => Ok((
            "get_neighbors",
            json!({
                "action": "get_neighbors",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "node_id": node_id,
            }),
        )),
        Command::CreateKnowledgePr { payload_file } => {
            let mut payload = read_payload_file(payload_file).await?;
            ensure_create_pr_action(&mut payload)?;
            Ok(("create_knowledge_pr", payload))
        }
    }
}

async fn read_payload_file(path: &PathBuf) -> Result<serde_json::Value, String> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("failed to read payload file {}: {error}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse payload file {}: {error}", path.display()))
}

fn ensure_create_pr_action(payload: &mut serde_json::Value) -> Result<(), String> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "payload file must contain a JSON object".to_string())?;
    match object.get("action").and_then(serde_json::Value::as_str) {
        Some("create_knowledge_pr") => Ok(()),
        Some(other) => Err(format!(
            "payload action must be create_knowledge_pr for this command, got {other}"
        )),
        None => {
            object.insert("action".to_string(), json!("create_knowledge_pr"));
            Ok(())
        }
    }
}

async fn run(command: Command) -> serde_json::Value {
    let tool = KnowledgeTool::new();
    let manager = ToolManager::new();
    manager.register(Arc::new(tool));

    let (action, request) = match payload_for_command(&command).await {
        Ok(payload) => payload,
        Err(error) => {
            return json!({
                "ok": false,
                "action": command.action_name(),
                "error": error,
            });
        }
    };
    let result = manager.execute("knowledge", request, make_ctx()).await;

    match result {
        Ok(result) => json!({
            "ok": true,
            "result": result,
        }),
        Err(error) => json!({
            "ok": false,
            "action": action,
            "error": error.to_string(),
        }),
    }
}

impl Command {
    fn action_name(&self) -> &'static str {
        match self {
            Self::ListRepos => "list_repos",
            Self::ResolveSnapshot { .. } => "resolve_snapshot",
            Self::ExploreTree { .. } => "explore_tree",
            Self::SearchNodes { .. } => "search_nodes",
            Self::GetNode { .. } => "get_node",
            Self::GetContent { .. } => "get_content",
            Self::GetNeighbors { .. } => "get_neighbors",
            Self::CreateKnowledgePr { .. } => "create_knowledge_pr",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, payload_for_command};
    use tempfile::NamedTempFile;

    fn write_payload(contents: &str) -> NamedTempFile {
        let file = NamedTempFile::new().expect("temp file should create");
        std::fs::write(file.path(), contents).expect("temp payload should write");
        file
    }

    #[tokio::test]
    async fn create_knowledge_pr_payload_file_injects_missing_action() {
        let file = write_payload(
            r#"{"target_repo":"acme/docs","pr_title":"t","pr_body":"b","files":[{"path":"docs/a.md","content":"x"}]}"#,
        );

        let (_, payload) = payload_for_command(&Command::CreateKnowledgePr {
            payload_file: file.path().to_path_buf(),
        })
        .await
        .expect("payload should load");

        assert_eq!(payload["action"], "create_knowledge_pr");
        assert_eq!(payload["target_repo"], "acme/docs");
    }

    #[tokio::test]
    async fn create_knowledge_pr_payload_file_accepts_matching_action() {
        let file = write_payload(
            r#"{"action":"create_knowledge_pr","target_repo":"acme/docs","pr_title":"t","pr_body":"b","files":[{"path":"docs/a.md","content":"x"}]}"#,
        );

        let (_, payload) = payload_for_command(&Command::CreateKnowledgePr {
            payload_file: file.path().to_path_buf(),
        })
        .await
        .expect("payload should load");

        assert_eq!(payload["action"], "create_knowledge_pr");
    }

    #[tokio::test]
    async fn create_knowledge_pr_payload_file_rejects_mismatched_action() {
        let file = write_payload(r#"{"action":"list_repos"}"#);

        let error = payload_for_command(&Command::CreateKnowledgePr {
            payload_file: file.path().to_path_buf(),
        })
        .await
        .unwrap_err();

        assert!(error.contains("create_knowledge_pr"));
        assert!(error.contains("list_repos"));
    }

    #[tokio::test]
    async fn create_knowledge_pr_payload_file_reports_invalid_json() {
        let file = write_payload("{not json");

        let error = payload_for_command(&Command::CreateKnowledgePr {
            payload_file: file.path().to_path_buf(),
        })
        .await
        .unwrap_err();

        assert!(error.contains("failed to parse payload file"));
    }

    #[tokio::test]
    async fn create_knowledge_pr_payload_file_reports_missing_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let missing_path = temp_dir.path().join("missing.json");

        let error = payload_for_command(&Command::CreateKnowledgePr {
            payload_file: missing_path,
        })
        .await
        .unwrap_err();

        assert!(error.contains("failed to read payload file"));
    }

    #[test]
    fn action_name_matches_create_knowledge_pr_command() {
        assert_eq!(
            Command::CreateKnowledgePr {
                payload_file: "request.json".into(),
            }
            .action_name(),
            "create_knowledge_pr"
        );
    }
}
