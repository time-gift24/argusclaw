use std::path::{Path, PathBuf};

use super::models::KnowledgeRepoDescriptor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRepoRegistry;

impl KnowledgeRepoRegistry {
    pub fn default_path_from_home(home: &Path) -> PathBuf {
        home.join(".arguswing").join("knowledge").join("repos.json")
    }

    pub fn load_default() -> Vec<KnowledgeRepoDescriptor> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };

        let path = Self::default_path_from_home(&home);
        let Ok(contents) = std::fs::read_to_string(path) else {
            return Vec::new();
        };

        serde_json::from_str::<Vec<KnowledgeRepoDescriptor>>(&contents).unwrap_or_default()
    }
}
