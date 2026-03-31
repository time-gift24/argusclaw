use std::sync::Arc;

use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;
use argus_tool::{KnowledgeTool, ToolManager};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

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
    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        pipe_tx,
        control_tx,
    })
}

fn payload_for_command(command: &Command) -> (&'static str, serde_json::Value) {
    match command {
        Command::ListRepos => (
            "list_repos",
            json!({ "action": "list_repos" }),
        ),
        Command::ResolveSnapshot { repo_id, r#ref } => (
            "resolve_snapshot",
            json!({
                "action": "resolve_snapshot",
                "repo_id": repo_id,
                "ref": r#ref,
            }),
        ),
        Command::ExploreTree {
            repo_id,
            snapshot_id,
            r#ref,
            path,
            depth,
        } => (
            "explore_tree",
            json!({
                "action": "explore_tree",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "path": path,
                "depth": depth,
            }),
        ),
        Command::SearchNodes {
            repo_id,
            snapshot_id,
            r#ref,
            query,
            scope_path,
            limit,
        } => (
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
        ),
        Command::GetNode {
            repo_id,
            snapshot_id,
            r#ref,
            node_id,
        } => (
            "get_node",
            json!({
                "action": "get_node",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "node_id": node_id,
            }),
        ),
        Command::GetContent {
            repo_id,
            snapshot_id,
            r#ref,
            node_id,
            max_chars,
            cursor,
        } => (
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
        ),
        Command::GetNeighbors {
            repo_id,
            snapshot_id,
            r#ref,
            node_id,
        } => (
            "get_neighbors",
            json!({
                "action": "get_neighbors",
                "repo_id": repo_id,
                "snapshot_id": snapshot_id,
                "ref": r#ref,
                "node_id": node_id,
            }),
        ),
    }
}

async fn run(command: Command) -> serde_json::Value {
    let tool = KnowledgeTool::new();
    let manager = ToolManager::new();
    manager.register(Arc::new(tool));

    let (action, request) = payload_for_command(&command);
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
