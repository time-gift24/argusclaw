mod cache;
mod cli;
mod error;
mod github;
mod indexer;
mod manifest;
mod markdown;
mod models;
mod pr;
mod provider;
mod registry;
mod tool;

pub use cache::SnapshotCache;
pub use cli::{CliOutput, CliRunner, RealCliRunner};
pub use error::KnowledgeToolError;
pub use github::{
    GitHubApiMethod, GitHubCommit, GitHubKnowledgeBackend, GitHubKnowledgeClient,
    GitHubTransport, ReqwestGitHubTransport,
};
pub use indexer::{KnowledgeBackend, KnowledgeIndexer};
pub use manifest::{DEFAULT_MANIFEST_PATHS, FileOverride, NodeOverride, RepositoryManifest};
pub use markdown::{ParsedSection, parse_markdown_sections};
pub use models::{
    ContentPage, ExploreTreeEntry, ExploreTreeResult, GitHubBlob, GitHubSnapshot, GitHubTree,
    GitHubTreeEntry, GitHubTreeEntryKind, KnowledgeAction, KnowledgeCreatePrArgs,
    KnowledgeCreatePrResult, KnowledgeFileWrite, KnowledgeManifestFilePatch,
    KnowledgeManifestNodePatch, KnowledgeManifestNodeSourcePatch, KnowledgeManifestPatch,
    KnowledgeManifestRepoPatch, KnowledgeNode, KnowledgeNodeKind, KnowledgeRelation,
    KnowledgeRepoDescriptor, KnowledgeSource, KnowledgeToolArgs,
};
pub use pr::{
    CliPrExecutor, GitPrExecutor, GitPrOutcome, KnowledgePrRemoteEntry, KnowledgePrRuntime,
    KnowledgePrService, KnowledgePrWorkspace, KnowledgePrWorkspaceFile, merge_manifest,
    serialize_manifest, validate_repo_relative_path,
};
pub use provider::{FileKnowledgeRepoProvider, StaticKnowledgeRepoProvider};
pub use registry::KnowledgeRepoRegistry;
pub use tool::{DefaultKnowledgeRuntime, KnowledgeRuntime, KnowledgeRuntimeBackend, KnowledgeTool};

#[cfg(test)]
mod tests {
    use super::{
        GitHubApiMethod, GitHubBlob, GitHubKnowledgeClient, GitHubTransport, GitHubTree,
        GitHubTreeEntry, GitHubTreeEntryKind, KnowledgeBackend, KnowledgeIndexer,
        KnowledgeRepoRegistry, KnowledgeRuntime, KnowledgeTool, KnowledgeToolArgs,
        RepositoryManifest, parse_markdown_sections,
    };
    use argus_protocol::NamedTool;
    use argus_protocol::ids::ThreadId;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::broadcast;

    #[derive(Debug)]
    struct FakeGitHubTransport {
        responses: Mutex<VecDeque<Value>>,
    }

    impl FakeGitHubTransport {
        fn with_json(responses: Vec<Value>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl GitHubTransport for FakeGitHubTransport {
        async fn request_json(
            &self,
            _method: GitHubApiMethod,
            _url: &str,
            _body: Option<Value>,
        ) -> Result<Value, super::KnowledgeToolError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| super::KnowledgeToolError::invalid_arguments("missing response"))
        }
    }

    fn make_ctx() -> Arc<argus_protocol::ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(16);
        let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(argus_protocol::ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
            pipe_tx,
            control_tx,
        })
    }

    #[derive(Default)]
    struct FakeKnowledgeRuntime {
        dispatch_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl KnowledgeRuntime for FakeKnowledgeRuntime {
        async fn dispatch(
            &self,
            _args: super::KnowledgeToolArgs,
            _ctx: Arc<argus_protocol::ToolExecutionContext>,
        ) -> Result<Value, argus_protocol::ToolError> {
            self.dispatch_calls.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"ok": true}))
        }
    }

    #[test]
    fn knowledge_scaffold_rejects_unknown_fields() {
        let err = KnowledgeToolArgs::parse(serde_json::json!({
            "action": "search_nodes",
            "repo_id": "acme-docs",
            "query": "refresh",
            "unexpected": true
        }))
        .unwrap_err();

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn knowledge_scaffold_resolve_snapshot_requires_repo_id() {
        let err = KnowledgeToolArgs::parse(serde_json::json!({
            "action": "resolve_snapshot"
        }))
        .unwrap_err();

        assert!(err.to_string().contains("repo_id"));
    }

    #[test]
    fn knowledge_scaffold_registry_default_path_uses_arguswing_home() {
        let path = KnowledgeRepoRegistry::default_path_from_home(Path::new("/tmp/home"));
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/home/.arguswing/knowledge/repos.json")
        );
    }

    #[test]
    fn knowledge_tool_definition_lists_expected_actions() {
        let tool = KnowledgeTool::new_for_test(FakeKnowledgeRuntime::default());
        let def = tool.definition();

        assert_eq!(def.name, "knowledge");
        assert!(def.description.contains("GitHub"));
        let parameters = def.parameters.to_string();
        assert!(parameters.contains("resolve_snapshot"));
        assert!(parameters.contains("search_nodes"));
        assert!(parameters.contains("create_knowledge_pr"));

        assert!(def.parameters["properties"].get("target_repo").is_some());
        assert!(def.parameters["properties"].get("files").is_some());
        assert!(def.parameters["properties"].get("manifest").is_some());
    }

    #[test]
    fn knowledge_tool_parses_create_knowledge_pr_payload() {
        let args = KnowledgeToolArgs::parse(serde_json::json!({
            "action": "create_knowledge_pr",
            "target_repo": "acme/docs",
            "base_ref": "main",
            "branch": "docs-update",
            "pr_title": "Update docs",
            "pr_body": "This updates the docs.",
            "draft": true,
            "files": [
                {
                    "path": "docs/guide.md",
                    "content": "# Guide\n"
                }
            ],
            "manifest": {
                "path": ".knowledge/repo.json",
                "repo": {
                    "title": "Docs",
                    "default_branch": "main",
                    "include": ["docs"],
                    "exclude": ["tmp"],
                    "entrypoints": ["README.md"]
                },
                "files": [
                    {
                        "path": "docs/guide.md",
                        "title": "Guide",
                        "summary": "Guide summary",
                        "tags": ["docs"],
                        "aliases": ["guide"]
                    }
                ],
                "nodes": [
                    {
                        "id": "docs/guide#intro",
                        "source": {
                            "path": "docs/guide.md",
                            "heading": "Intro"
                        },
                        "title": "Intro",
                        "summary": "Intro summary",
                        "tags": ["docs"],
                        "aliases": ["intro"],
                        "relations": [
                            {
                                "type": "related",
                                "target": "docs/api#intro"
                            }
                        ]
                    }
                ]
            }
        }))
        .unwrap();

        assert!(matches!(
            args.action,
            super::KnowledgeAction::CreateKnowledgePr
        ));
        assert_eq!(args.target_repo.as_deref(), Some("acme/docs"));
        assert_eq!(args.base_ref.as_deref(), Some("main"));
        assert_eq!(args.branch.as_deref(), Some("docs-update"));
        assert_eq!(args.pr_title.as_deref(), Some("Update docs"));
        assert_eq!(args.pr_body.as_deref(), Some("This updates the docs."));
        assert_eq!(args.draft, Some(true));
        assert_eq!(args.files.len(), 1);
        assert_eq!(args.files[0].path, "docs/guide.md");
        assert_eq!(args.files[0].content, "# Guide\n");
        assert_eq!(
            args.manifest
                .as_ref()
                .and_then(|manifest| manifest.path.as_deref()),
            Some(".knowledge/repo.json")
        );
    }

    #[test]
    fn knowledge_tool_rejects_malformed_create_knowledge_pr_payload() {
        let err = KnowledgeToolArgs::parse(serde_json::json!({
            "action": "create_knowledge_pr",
            "target_repo": "acme/docs",
            "pr_title": "Update docs",
            "pr_body": "This updates the docs.",
            "files": [
                {
                    "path": "docs/guide.md"
                }
            ]
        }))
        .unwrap_err();

        assert!(err.to_string().contains("content"));
    }

    #[test]
    fn knowledge_tool_risk_level_is_medium() {
        let tool = KnowledgeTool::new_for_test(FakeKnowledgeRuntime::default());

        assert_eq!(
            tool.risk_level(),
            argus_protocol::risk_level::RiskLevel::Medium
        );
    }

    #[tokio::test]
    async fn knowledge_tool_rejects_invalid_action_before_runtime() {
        let runtime = FakeKnowledgeRuntime::default();
        let calls = runtime.dispatch_calls.clone();
        let tool = KnowledgeTool::new_for_test(runtime);

        let err = tool
            .execute(
                serde_json::json!({ "action": "unknown_action" }),
                make_ctx(),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("unknown variant"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn knowledge_github_resolve_snapshot_parses_head_commit() {
        let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
            serde_json::json!({ "object": { "sha": "abc123" } }),
        ]));

        let snapshot = client
            .resolve_snapshot("acme", "docs", "main")
            .await
            .unwrap();
        assert_eq!(snapshot.rev, "abc123");
    }

    #[tokio::test]
    async fn knowledge_github_read_tree_maps_entries() {
        let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
            serde_json::json!({
                "sha": "abc123",
                "tree": { "sha": "tree-1" }
            }),
            serde_json::json!({
                "tree": [
                    { "path": "README.md", "type": "blob", "sha": "blob-1" },
                    { "path": "docs", "type": "tree", "sha": "tree-1" }
                ]
            }),
        ]));

        let tree = client.read_tree("acme", "docs", "abc123").await.unwrap();
        assert_eq!(tree.entries.len(), 2);
    }

    #[tokio::test]
    async fn knowledge_github_read_tree_resolves_commit_to_tree_sha() {
        let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
            serde_json::json!({
                "sha": "abc123",
                "tree": { "sha": "tree-1" }
            }),
            serde_json::json!({
                "tree": [
                    { "path": "README.md", "type": "blob", "sha": "blob-1" },
                    { "path": "docs", "type": "tree", "sha": "tree-2" }
                ]
            }),
        ]));

        let tree = client.read_tree("acme", "docs", "abc123").await.unwrap();
        assert_eq!(tree.entries.len(), 2);
    }

    #[tokio::test]
    async fn knowledge_github_read_blob_decodes_base64() {
        let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
            serde_json::json!({
                "sha": "blob-1",
                "content": "IyBUaXRsZQoKQm9keQ==",
                "encoding": "base64"
            }),
        ]));

        let blob = client.read_blob("acme", "docs", "blob-1").await.unwrap();
        assert!(blob.text.contains("# Title"));
    }

    #[test]
    fn knowledge_manifest_overrides_file_title_and_aliases() {
        let manifest = RepositoryManifest::from_json(serde_json::json!({
            "version": 1,
            "files": [
                {
                    "path": "README.md",
                    "title": "Overview",
                    "aliases": ["home"]
                }
            ]
        }))
        .unwrap();

        let meta = manifest.file_override("README.md").unwrap();
        assert_eq!(meta.title.as_deref(), Some("Overview"));
        assert_eq!(meta.aliases, vec!["home"]);
    }

    #[test]
    fn knowledge_manifest_extracts_markdown_sections_with_line_spans() {
        let sections = parse_markdown_sections(
            "docs/auth.md",
            "# Auth\nintro\n## Refresh Flow\nbody\n## Login Flow\nbody\n",
        );

        assert_eq!(sections[0].anchor, "auth");
        assert_eq!(sections[1].start_line, 3);
        assert_eq!(sections[1].end_line, 4);
    }

    #[test]
    fn knowledge_manifest_declared_id_wins_over_generated_id() {
        let manifest = RepositoryManifest::from_json(serde_json::json!({
            "version": 1,
            "nodes": [
                {
                    "id": "auth/refresh-flow",
                    "source": { "path": "docs/auth.md", "heading": "Refresh Flow" }
                }
            ]
        }))
        .unwrap();

        let node_id = manifest.resolve_section_id(
            "docs/auth.md",
            "Refresh Flow",
            "docs/auth.md#refresh-flow",
        );
        assert_eq!(node_id, "auth/refresh-flow");
    }

    #[derive(Clone)]
    struct RecordingKnowledgeBackend {
        tree: GitHubTree,
        manifest: Option<RepositoryManifest>,
        blobs: HashMap<String, GitHubBlob>,
        blob_fetches: Arc<AtomicUsize>,
    }

    impl RecordingKnowledgeBackend {
        fn tree_only() -> Self {
            Self {
                tree: GitHubTree {
                    rev: "abc123".to_string(),
                    entries: vec![
                        GitHubTreeEntry {
                            path: "docs/auth.md".to_string(),
                            sha: "blob-auth".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Blob,
                        },
                        GitHubTreeEntry {
                            path: "docs/login.md".to_string(),
                            sha: "blob-login".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Blob,
                        },
                    ],
                },
                manifest: None,
                blobs: HashMap::new(),
                blob_fetches: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn with_auth_docs() -> Self {
            let mut backend = Self::tree_only();
            backend.manifest = Some(
                RepositoryManifest::from_json(serde_json::json!({
                    "version": 1,
                    "nodes": [
                        {
                            "id": "docs/auth.md#refresh-flow",
                            "source": { "path": "docs/auth.md", "heading": "Refresh Flow" },
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
                .unwrap(),
            );
            backend.blobs.insert(
                "docs/auth.md".to_string(),
                GitHubBlob {
                    sha: "blob-auth".to_string(),
                    text: "# Auth\n## Refresh Flow\nToken refresh details\n".to_string(),
                },
            );
            backend.blobs.insert(
                "docs/login.md".to_string(),
                GitHubBlob {
                    sha: "blob-login".to_string(),
                    text: "# Login Flow\nLogin details\n".to_string(),
                },
            );
            backend
        }

        fn with_nested_tree() -> Self {
            Self {
                tree: GitHubTree {
                    rev: "abc123".to_string(),
                    entries: vec![
                        GitHubTreeEntry {
                            path: "README.md".to_string(),
                            sha: "blob-readme".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Blob,
                        },
                        GitHubTreeEntry {
                            path: "docs".to_string(),
                            sha: "tree-docs".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Tree,
                        },
                        GitHubTreeEntry {
                            path: "docs/auth.md".to_string(),
                            sha: "blob-auth".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Blob,
                        },
                        GitHubTreeEntry {
                            path: "docs/guides".to_string(),
                            sha: "tree-guides".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Tree,
                        },
                        GitHubTreeEntry {
                            path: "docs/guides/setup.md".to_string(),
                            sha: "blob-setup".to_string(),
                            mode: None,
                            kind: GitHubTreeEntryKind::Blob,
                        },
                    ],
                },
                manifest: None,
                blobs: HashMap::new(),
                blob_fetches: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn with_large_section() -> Self {
            let mut backend = Self::with_auth_docs();
            backend.blobs.insert(
                "docs/auth.md".to_string(),
                GitHubBlob {
                    sha: "blob-auth".to_string(),
                    text: format!("# Auth\n## Refresh Flow\n{}\n", "token ".repeat(200)),
                },
            );
            backend
        }

        fn blob_fetch_count(&self) -> usize {
            self.blob_fetches.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl KnowledgeBackend for RecordingKnowledgeBackend {
        async fn read_tree(
            &self,
            _snapshot_id: &str,
        ) -> Result<GitHubTree, super::KnowledgeToolError> {
            Ok(self.tree.clone())
        }

        async fn read_manifest(
            &self,
            _snapshot_id: &str,
        ) -> Result<Option<RepositoryManifest>, super::KnowledgeToolError> {
            Ok(self.manifest.clone())
        }

        async fn read_blob(
            &self,
            _snapshot_id: &str,
            path: &str,
            _sha: &str,
        ) -> Result<GitHubBlob, super::KnowledgeToolError> {
            self.blob_fetches.fetch_add(1, Ordering::SeqCst);
            self.blobs
                .get(path)
                .cloned()
                .ok_or_else(|| super::KnowledgeToolError::NotFound(path.to_string()))
        }
    }

    #[tokio::test]
    async fn knowledge_index_explore_tree_lists_entries_without_blob_fetch() {
        let backend = RecordingKnowledgeBackend::tree_only();
        let indexer = KnowledgeIndexer::new(backend.clone());

        let tree = indexer.explore_tree("snap-1", "/docs", 1).await.unwrap();
        assert_eq!(tree.entries.len(), 2);
        assert_eq!(backend.blob_fetch_count(), 0);
    }

    #[tokio::test]
    async fn knowledge_index_explore_tree_includes_directories_with_child_counts() {
        let backend = RecordingKnowledgeBackend::with_nested_tree();
        let indexer = KnowledgeIndexer::new(backend);

        let tree = indexer.explore_tree("snap-1", "/", 1).await.unwrap();

        assert!(
            tree.entries
                .iter()
                .any(|entry| entry.path == "/docs" && entry.child_count == 2)
        );
        assert!(
            tree.entries
                .iter()
                .any(|entry| entry.path == "/README.md" && entry.child_count == 0)
        );
    }

    #[tokio::test]
    async fn knowledge_index_search_matches_manifest_and_heading_metadata() {
        let backend = RecordingKnowledgeBackend::with_auth_docs();
        let indexer = KnowledgeIndexer::new(backend);

        let results = indexer
            .search_nodes("snap-1", "token refresh", Some("/docs"), 8)
            .await
            .unwrap();

        assert!(results.iter().any(|node| node.title == "Refresh Flow"));
    }

    #[tokio::test]
    async fn knowledge_index_get_content_is_bounded_and_cursorized() {
        let backend = RecordingKnowledgeBackend::with_large_section();
        let indexer = KnowledgeIndexer::new(backend);

        let page = indexer
            .get_content("snap-1", "docs/auth.md#refresh-flow", Some(120), None)
            .await
            .unwrap();

        assert!(page.truncated);
        assert!(page.next_cursor.is_some());
    }

    #[tokio::test]
    async fn knowledge_index_get_neighbors_returns_manifest_relations() {
        let backend = RecordingKnowledgeBackend::with_auth_docs();
        let indexer = KnowledgeIndexer::new(backend);

        let neighbors = indexer
            .get_neighbors("snap-1", "docs/auth.md#refresh-flow")
            .await
            .unwrap();

        assert!(neighbors.iter().any(|node| node.id == "auth/login-flow"));
    }
}
