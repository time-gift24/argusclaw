mod error;
mod models;
mod registry;

pub use error::KnowledgeToolError;
pub use models::{KnowledgeAction, KnowledgeRepoDescriptor, KnowledgeToolArgs};
pub use registry::KnowledgeRepoRegistry;

#[cfg(test)]
mod tests {
    use super::{KnowledgeRepoRegistry, KnowledgeToolArgs};
    use std::path::Path;

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
}
