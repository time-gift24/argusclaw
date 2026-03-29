mod error;
mod github;
mod models;
mod registry;

pub use error::KnowledgeToolError;
pub use github::{GitHubKnowledgeClient, GitHubTransport, ReqwestGitHubTransport};
pub use models::{
    GitHubBlob, GitHubSnapshot, GitHubTree, GitHubTreeEntry, GitHubTreeEntryKind,
    KnowledgeAction, KnowledgeRepoDescriptor, KnowledgeToolArgs,
};
pub use registry::KnowledgeRepoRegistry;

#[cfg(test)]
mod tests {
    use super::{
        GitHubKnowledgeClient, GitHubTransport, KnowledgeRepoRegistry, KnowledgeToolArgs,
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
}
