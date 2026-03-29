use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRepoRegistry;

impl KnowledgeRepoRegistry {
    pub fn default_path_from_home(home: &Path) -> PathBuf {
        home.join(".arguswing").join("knowledge").join("repos.json")
    }
}
