//! Knowledge repo provider implementations.

use argus_protocol::ids::AgentId;
use argus_protocol::{KnowledgeRepoProvider, KnowledgeRepoRecord};
use async_trait::async_trait;

use super::error::KnowledgeToolError;
use super::registry::KnowledgeRepoRegistry;

/// File-based provider wrapping existing `KnowledgeRepoRegistry`.
///
/// Used by CLI binary and tests for backward compatibility.
/// Ignores `agent_id` — all repos are visible.
pub struct FileKnowledgeRepoProvider;

#[async_trait]
impl KnowledgeRepoProvider for FileKnowledgeRepoProvider {
    async fn list_repos(
        &self,
        _agent_id: Option<AgentId>,
    ) -> Result<Vec<KnowledgeRepoRecord>, Box<dyn std::error::Error + Send + Sync>> {
        let repos = KnowledgeRepoRegistry::load_default();
        Ok(repos
            .into_iter()
            .enumerate()
            .map(|(index, repo)| KnowledgeRepoRecord {
                id: index as i64,
                repo: format!("{}/{}", repo.owner, repo.name),
                repo_id: repo.repo_id,
                provider: repo.provider,
                owner: repo.owner,
                name: repo.name,
                default_branch: repo.default_branch,
                manifest_paths: repo.manifest_paths,
                workspace: String::new(),
            })
            .collect())
    }

    async fn get_repo(
        &self,
        repo: &str,
        _agent_id: Option<AgentId>,
    ) -> Result<KnowledgeRepoRecord, Box<dyn std::error::Error + Send + Sync>> {
        let repos = KnowledgeRepoRegistry::load_default();
        let descriptor = repos
            .into_iter()
            .find(|r| format!("{}/{}", r.owner, r.name) == repo || r.repo_id == repo)
            .ok_or_else(|| KnowledgeToolError::NotFound(repo.to_string()))?;
        Ok(KnowledgeRepoRecord {
            id: 0,
            repo: format!("{}/{}", descriptor.owner, descriptor.name),
            repo_id: descriptor.repo_id,
            provider: descriptor.provider,
            owner: descriptor.owner,
            name: descriptor.name,
            default_branch: descriptor.default_branch,
            manifest_paths: descriptor.manifest_paths,
            workspace: String::new(),
        })
    }
}

/// Static provider for unit tests. Ignores `agent_id`.
pub struct StaticKnowledgeRepoProvider {
    repos: Vec<KnowledgeRepoRecord>,
}

impl StaticKnowledgeRepoProvider {
    #[must_use]
    pub fn new(repos: Vec<KnowledgeRepoRecord>) -> Self {
        Self { repos }
    }
}

#[async_trait]
impl KnowledgeRepoProvider for StaticKnowledgeRepoProvider {
    async fn list_repos(
        &self,
        _agent_id: Option<AgentId>,
    ) -> Result<Vec<KnowledgeRepoRecord>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.repos.clone())
    }

    async fn get_repo(
        &self,
        repo: &str,
        _agent_id: Option<AgentId>,
    ) -> Result<KnowledgeRepoRecord, Box<dyn std::error::Error + Send + Sync>> {
        self.repos
            .iter()
            .find(|r| r.repo == repo || r.repo_id == repo)
            .cloned()
            .ok_or_else(|| KnowledgeToolError::NotFound(repo.to_string()).into())
    }
}
