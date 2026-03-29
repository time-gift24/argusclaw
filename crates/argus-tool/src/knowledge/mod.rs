mod error;
mod github;
mod manifest;
mod markdown;
mod models;
mod registry;

pub use error::KnowledgeToolError;
pub use github::{GitHubKnowledgeClient, GitHubTransport, ReqwestGitHubTransport};
pub use manifest::{FileOverride, NodeOverride, RepositoryManifest, DEFAULT_MANIFEST_PATHS};
pub use markdown::{parse_markdown_sections, ParsedSection};
pub use models::{
    GitHubBlob, GitHubSnapshot, GitHubTree, GitHubTreeEntry, GitHubTreeEntryKind,
    KnowledgeAction, KnowledgeRelation, KnowledgeRepoDescriptor, KnowledgeToolArgs,
};
pub use registry::KnowledgeRepoRegistry;

#[cfg(test)]
mod tests {
    use super::{
        parse_markdown_sections, GitHubKnowledgeClient, GitHubTransport, KnowledgeRepoRegistry,
        KnowledgeToolArgs, RepositoryManifest,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Mutex;

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
        async fn get_json(&self, _url: &str) -> Result<Value, super::KnowledgeToolError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| super::KnowledgeToolError::invalid_arguments("missing response"))
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

    #[tokio::test]
    async fn knowledge_github_resolve_snapshot_parses_head_commit() {
        let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
            serde_json::json!({ "object": { "sha": "abc123" } }),
        ]));

        let snapshot = client.resolve_snapshot("acme", "docs", "main").await.unwrap();
        assert_eq!(snapshot.rev, "abc123");
    }

    #[tokio::test]
    async fn knowledge_github_read_tree_maps_entries() {
        let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
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

        let node_id =
            manifest.resolve_section_id("docs/auth.md", "Refresh Flow", "docs/auth.md#refresh-flow");
        assert_eq!(node_id, "auth/refresh-flow");
    }
}
