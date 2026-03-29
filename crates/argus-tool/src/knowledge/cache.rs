use dashmap::DashMap;

use super::manifest::RepositoryManifest;
use super::models::{GitHubTree, KnowledgeNode};

pub struct SnapshotCache {
    pub tree: GitHubTree,
    pub manifest: Option<RepositoryManifest>,
    pub nodes: DashMap<String, KnowledgeNode>,
}

impl SnapshotCache {
    pub fn new(tree: GitHubTree, manifest: Option<RepositoryManifest>) -> Self {
        Self {
            tree,
            manifest,
            nodes: DashMap::new(),
        }
    }
}
