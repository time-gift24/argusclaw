use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::broadcast;

use argus_protocol::ids::ThreadId;
use argus_protocol::{NamedTool, ToolExecutionContext};
use argus_tool::knowledge::{
    DefaultKnowledgeRuntime, GitHubBlob, GitHubSnapshot, GitHubTree, GitHubTreeEntry,
    GitHubTreeEntryKind, KnowledgeBackend, KnowledgeRepoDescriptor, KnowledgeRuntimeBackend,
    KnowledgeTool, KnowledgeToolError, RepositoryManifest,
};

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (pipe_tx, _) = broadcast::channel(16);
    let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
        control_tx,
    })
}

fn repo_descriptor() -> KnowledgeRepoDescriptor {
    KnowledgeRepoDescriptor {
        repo_id: "acme-docs".to_string(),
        provider: "github".to_string(),
        owner: "acme".to_string(),
        name: "docs".to_string(),
        default_branch: "main".to_string(),
        manifest_paths: vec!["knowledge.json".to_string()],
    }
}

#[derive(Clone)]
struct FixedKnowledgeBackend {
    repos: Vec<KnowledgeRepoDescriptor>,
    snapshot_id: String,
    snapshot: GitHubSnapshot,
    tree: GitHubTree,
    manifest: Option<RepositoryManifest>,
    blobs: HashMap<String, GitHubBlob>,
}

impl FixedKnowledgeBackend {
    fn with_manifest() -> Self {
        let manifest = RepositoryManifest::from_json(json!({
            "version": 1,
            "files": [
                {
                    "path": "docs/auth.md",
                    "title": "Auth Guide",
                    "summary": "Authentication entry point",
                    "aliases": ["auth"],
                    "tags": ["security"]
                }
            ],
            "nodes": [
                {
                    "id": "auth/refresh-flow",
                    "source": { "path": "docs/auth.md", "heading": "Refresh Flow" },
                    "title": "Refresh Flow",
                    "summary": "Token refresh details",
                    "relations": [
                        { "type": "related", "target": "auth/login-flow" }
                    ]
                },
                {
                    "id": "auth/login-flow",
                    "source": { "path": "docs/login.md", "heading": "Login Flow" }
                }
            ]
        }))
        .expect("manifest should parse");

        let mut blobs = HashMap::new();
        blobs.insert(
            "README.md".to_string(),
            GitHubBlob {
                sha: "blob-readme".to_string(),
                text: "# Overview\nRepository overview.\n".to_string(),
            },
        );
        blobs.insert(
            "docs/auth.md".to_string(),
            GitHubBlob {
                sha: "blob-auth".to_string(),
                text: "# Auth\nIntro\n## Refresh Flow\nToken refresh details.\n## Login Flow\nLogin details.\n"
                    .to_string(),
            },
        );
        blobs.insert(
            "docs/login.md".to_string(),
            GitHubBlob {
                sha: "blob-login".to_string(),
                text: "# Login Flow\nLogin details.\n".to_string(),
            },
        );
        blobs.insert(
            "knowledge.json".to_string(),
            GitHubBlob {
                sha: "blob-manifest".to_string(),
                text: json!({
                    "version": 1,
                    "files": [
                        {
                            "path": "docs/auth.md",
                            "title": "Auth Guide",
                            "summary": "Authentication entry point",
                            "aliases": ["auth"],
                            "tags": ["security"]
                        }
                    ],
                    "nodes": [
                        {
                            "id": "auth/refresh-flow",
                            "source": { "path": "docs/auth.md", "heading": "Refresh Flow" },
                            "title": "Refresh Flow",
                            "summary": "Token refresh details",
                            "relations": [
                                { "type": "related", "target": "auth/login-flow" }
                            ]
                        },
                        {
                            "id": "auth/login-flow",
                            "source": { "path": "docs/login.md", "heading": "Login Flow" }
                        }
                    ]
                })
                .to_string(),
            },
        );

        Self {
            repos: vec![repo_descriptor()],
            snapshot_id: "snap-acme-docs".to_string(),
            snapshot: GitHubSnapshot {
                owner: "acme".to_string(),
                repo: "docs".to_string(),
                ref_name: "main".to_string(),
                rev: "abc123".to_string(),
            },
            tree: GitHubTree {
                rev: "abc123".to_string(),
                entries: vec![
                    GitHubTreeEntry {
                        path: "README.md".to_string(),
                        sha: "blob-readme".to_string(),
                        kind: GitHubTreeEntryKind::Blob,
                    },
                    GitHubTreeEntry {
                        path: "docs".to_string(),
                        sha: "tree-docs".to_string(),
                        kind: GitHubTreeEntryKind::Tree,
                    },
                    GitHubTreeEntry {
                        path: "docs/auth.md".to_string(),
                        sha: "blob-auth".to_string(),
                        kind: GitHubTreeEntryKind::Blob,
                    },
                    GitHubTreeEntry {
                        path: "docs/login.md".to_string(),
                        sha: "blob-login".to_string(),
                        kind: GitHubTreeEntryKind::Blob,
                    },
                    GitHubTreeEntry {
                        path: "knowledge.json".to_string(),
                        sha: "blob-manifest".to_string(),
                        kind: GitHubTreeEntryKind::Blob,
                    },
                ],
            },
            manifest: Some(manifest),
            blobs,
        }
    }

    fn without_manifest() -> Self {
        let mut backend = Self::with_manifest();
        backend.manifest = None;
        backend.blobs.remove("knowledge.json");
        backend
    }
}

#[async_trait]
impl KnowledgeRuntimeBackend for FixedKnowledgeBackend {
    async fn list_repos(&self) -> Result<Vec<KnowledgeRepoDescriptor>, KnowledgeToolError> {
        Ok(self.repos.clone())
    }

    fn repo_descriptor(&self, repo_id: &str) -> Option<KnowledgeRepoDescriptor> {
        self.repos
            .iter()
            .find(|repo| repo.repo_id == repo_id)
            .cloned()
    }

    async fn resolve_snapshot(
        &self,
        repo_id: &str,
        ref_name: &str,
    ) -> Result<(String, GitHubSnapshot), KnowledgeToolError> {
        assert_eq!(repo_id, "acme-docs");
        assert_eq!(ref_name, "main");
        Ok((self.snapshot_id.clone(), self.snapshot.clone()))
    }
}

#[async_trait]
impl KnowledgeBackend for FixedKnowledgeBackend {
    async fn read_tree(&self, snapshot_id: &str) -> Result<GitHubTree, KnowledgeToolError> {
        assert_eq!(snapshot_id, self.snapshot_id);
        Ok(self.tree.clone())
    }

    async fn read_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<RepositoryManifest>, KnowledgeToolError> {
        assert_eq!(snapshot_id, self.snapshot_id);
        Ok(self.manifest.clone())
    }

    async fn read_blob(
        &self,
        snapshot_id: &str,
        path: &str,
        _sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError> {
        assert_eq!(snapshot_id, self.snapshot_id);
        self.blobs
            .get(path)
            .cloned()
            .ok_or_else(|| KnowledgeToolError::NotFound(path.to_string()))
    }
}

fn make_fake_knowledge_tool() -> KnowledgeTool<DefaultKnowledgeRuntime<FixedKnowledgeBackend>> {
    let runtime = DefaultKnowledgeRuntime::new_for_test(FixedKnowledgeBackend::with_manifest());
    KnowledgeTool::new_for_test(runtime)
}

fn make_fake_knowledge_tool_without_manifest()
-> KnowledgeTool<DefaultKnowledgeRuntime<FixedKnowledgeBackend>> {
    let runtime = DefaultKnowledgeRuntime::new_for_test(FixedKnowledgeBackend::without_manifest());
    KnowledgeTool::new_for_test(runtime)
}

#[tokio::test]
async fn knowledge_flow_progressive_read_path_works_end_to_end() {
    let tool = make_fake_knowledge_tool();

    let snapshot = tool
        .execute(
            json!({ "action": "resolve_snapshot", "repo_id": "acme-docs" }),
            make_ctx(),
        )
        .await
        .unwrap();
    let snapshot_id = snapshot["snapshot_id"].as_str().unwrap();
    assert_eq!(snapshot_id, "snap-acme-docs");

    let tree = tool
        .execute(
            json!({ "action": "explore_tree", "snapshot_id": snapshot_id, "path": "/", "depth": 2 }),
            make_ctx(),
        )
        .await
        .unwrap();
    assert!(!tree["entries"].as_array().unwrap().is_empty());

    let hits = tool
        .execute(
            json!({ "action": "search_nodes", "snapshot_id": snapshot_id, "query": "refresh flow" }),
            make_ctx(),
        )
        .await
        .unwrap();
    assert!(!hits["results"].as_array().unwrap().is_empty());

    let node = tool
        .execute(
            json!({ "action": "get_node", "snapshot_id": snapshot_id, "node_id": "auth/refresh-flow" }),
            make_ctx(),
        )
        .await
        .unwrap();
    assert!(node.to_string().contains("\"path\":\"docs/auth.md\""));

    let content = tool
        .execute(
            json!({ "action": "get_content", "snapshot_id": snapshot_id, "node_id": "auth/refresh-flow", "max_chars": 120 }),
            make_ctx(),
        )
        .await
        .unwrap();
    assert!(
        content["content"]
            .as_str()
            .unwrap()
            .contains("Token refresh")
    );

    let neighbors = tool
        .execute(
            json!({ "action": "get_neighbors", "snapshot_id": snapshot_id, "node_id": "auth/refresh-flow" }),
            make_ctx(),
        )
        .await
        .unwrap();
    assert!(!neighbors["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn knowledge_flow_missing_manifest_falls_back_to_convention() {
    let tool = make_fake_knowledge_tool_without_manifest();

    let snapshot = tool
        .execute(
            json!({ "action": "resolve_snapshot", "repo_id": "acme-docs" }),
            make_ctx(),
        )
        .await
        .unwrap();
    let snapshot_id = snapshot["snapshot_id"].as_str().unwrap();

    let result = tool
        .execute(
            json!({ "action": "search_nodes", "snapshot_id": snapshot_id, "query": "Auth" }),
            make_ctx(),
        )
        .await
        .unwrap();

    assert!(!result["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn knowledge_flow_public_api_never_exposes_local_repo_path() {
    let tool = make_fake_knowledge_tool();

    let snapshot = tool
        .execute(
            json!({ "action": "resolve_snapshot", "repo_id": "acme-docs" }),
            make_ctx(),
        )
        .await
        .unwrap();
    let snapshot_id = snapshot["snapshot_id"].as_str().unwrap();

    let result = tool
        .execute(
            json!({ "action": "get_node", "snapshot_id": snapshot_id, "node_id": "auth/refresh-flow" }),
            make_ctx(),
        )
        .await
        .unwrap();

    let rendered = result.to_string();
    assert!(rendered.contains("\"path\":\"docs/auth.md\""));
    assert!(!rendered.contains("/Users/"));
    assert!(!rendered.contains(".worktrees"));
}
