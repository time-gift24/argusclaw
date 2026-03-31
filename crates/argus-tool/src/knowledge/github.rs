use async_trait::async_trait;
use base64::Engine;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;

use super::error::KnowledgeToolError;
use super::manifest::{DEFAULT_MANIFEST_PATHS, RepositoryManifest};
use super::models::{GitHubBlob, GitHubSnapshot, GitHubTree, GitHubTreeEntry, GitHubTreeEntryKind};
use super::tool::KnowledgeRuntimeBackend;
use super::{KnowledgeBackend, KnowledgeRepoDescriptor};

#[async_trait]
pub trait GitHubTransport: Send + Sync {
    async fn get_json(&self, url: &str) -> Result<Value, KnowledgeToolError>;
}

pub struct ReqwestGitHubTransport {
    client: reqwest::Client,
}

impl ReqwestGitHubTransport {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ReqwestGitHubTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GitHubTransport for ReqwestGitHubTransport {
    async fn get_json(&self, url: &str) -> Result<Value, KnowledgeToolError> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|err| KnowledgeToolError::RequestFailed(err.to_string()))?;

        let status = response.status();
        let rate_limit_remaining = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(KnowledgeToolError::NotFound(url.to_string()));
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || (status == reqwest::StatusCode::FORBIDDEN && rate_limit_remaining == "0")
        {
            return Err(KnowledgeToolError::RateLimited(url.to_string()));
        }

        if !status.is_success() {
            return Err(KnowledgeToolError::RequestFailed(format!(
                "{} returned {}",
                url, status
            )));
        }

        response
            .json()
            .await
            .map_err(|err| KnowledgeToolError::RequestFailed(err.to_string()))
    }
}

pub struct GitHubKnowledgeClient<T: GitHubTransport> {
    transport: T,
}

impl<T: GitHubTransport> GitHubKnowledgeClient<T> {
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    #[must_use]
    pub fn new_for_test(transport: T) -> Self {
        Self::new(transport)
    }

    pub async fn resolve_snapshot(
        &self,
        owner: &str,
        repo: &str,
        ref_name: &str,
    ) -> Result<GitHubSnapshot, KnowledgeToolError> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/git/ref/heads/{ref_name}");
        let value = self.transport.get_json(&url).await?;
        check_api_error(&value)?;

        let response: GitHubRefResponse = serde_json::from_value(value)
            .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;

        Ok(GitHubSnapshot {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_name: ref_name.to_string(),
            rev: response.object.sha,
        })
    }

    pub async fn read_tree(
        &self,
        owner: &str,
        repo: &str,
        rev: &str,
    ) -> Result<GitHubTree, KnowledgeToolError> {
        let commit_url = format!("https://api.github.com/repos/{owner}/{repo}/git/commits/{rev}");
        let commit_value = self.transport.get_json(&commit_url).await?;
        check_api_error(&commit_value)?;

        let commit_response: GitHubCommitResponse = serde_json::from_value(commit_value)
            .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;

        let tree_url = format!(
            "https://api.github.com/repos/{owner}/{repo}/git/trees/{}?recursive=1",
            commit_response.tree.sha
        );
        let value = self.transport.get_json(&tree_url).await?;
        check_api_error(&value)?;

        let response: GitHubTreeResponse = serde_json::from_value(value)
            .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;

        Ok(GitHubTree {
            rev: rev.to_string(),
            entries: response
                .tree
                .into_iter()
                .map(|entry| GitHubTreeEntry {
                    path: entry.path,
                    sha: entry.sha,
                    kind: GitHubTreeEntryKind::from_api(&entry.kind),
                })
                .collect(),
        })
    }

    pub async fn read_blob(
        &self,
        owner: &str,
        repo: &str,
        blob_sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/git/blobs/{blob_sha}");
        let value = self.transport.get_json(&url).await?;
        check_api_error(&value)?;

        let response: GitHubBlobResponse = serde_json::from_value(value)
            .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;

        if response.encoding != "base64" {
            return Err(KnowledgeToolError::unexpected_response(format!(
                "unsupported blob encoding {}",
                response.encoding
            )));
        }

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(response.content.replace('\n', ""))
            .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;
        let text = String::from_utf8(decoded)
            .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;

        Ok(GitHubBlob {
            sha: response.sha,
            text,
        })
    }
}

#[derive(Debug, Clone)]
struct SnapshotRecord {
    repo: KnowledgeRepoDescriptor,
    snapshot: GitHubSnapshot,
}

pub struct GitHubKnowledgeBackend<T: GitHubTransport> {
    client: GitHubKnowledgeClient<T>,
    repos: DashMap<String, KnowledgeRepoDescriptor>,
    snapshots: DashMap<String, SnapshotRecord>,
}

impl<T: GitHubTransport> GitHubKnowledgeBackend<T> {
    #[must_use]
    pub fn new(repos: Vec<KnowledgeRepoDescriptor>, transport: T) -> Self {
        Self::with_client(GitHubKnowledgeClient::new(transport), repos)
    }

    #[must_use]
    pub fn with_client(
        client: GitHubKnowledgeClient<T>,
        repos: Vec<KnowledgeRepoDescriptor>,
    ) -> Self {
        let repos = repos
            .into_iter()
            .map(|repo| (repo.repo_id.clone(), repo))
            .collect::<DashMap<_, _>>();
        Self {
            client,
            repos,
            snapshots: DashMap::new(),
        }
    }
}

#[async_trait]
impl<T: GitHubTransport> KnowledgeRuntimeBackend for GitHubKnowledgeBackend<T> {
    async fn list_repos(&self) -> Result<Vec<KnowledgeRepoDescriptor>, KnowledgeToolError> {
        Ok(self
            .repos
            .iter()
            .map(|entry| entry.value().clone())
            .collect())
    }

    fn repo_descriptor(&self, repo_id: &str) -> Option<KnowledgeRepoDescriptor> {
        self.repos.get(repo_id).map(|entry| entry.value().clone())
    }

    async fn resolve_snapshot(
        &self,
        repo_id: &str,
        ref_name: &str,
    ) -> Result<(String, GitHubSnapshot), KnowledgeToolError> {
        let repo = self
            .repo_descriptor(repo_id)
            .ok_or_else(|| KnowledgeToolError::NotFound(repo_id.to_string()))?;

        let snapshot = self
            .client
            .resolve_snapshot(&repo.owner, &repo.name, ref_name)
            .await?;
        let snapshot_id = format!("{repo_id}@{}", snapshot.rev);
        self.snapshots.insert(
            snapshot_id.clone(),
            SnapshotRecord {
                repo,
                snapshot: snapshot.clone(),
            },
        );

        Ok((snapshot_id, snapshot))
    }
}

#[async_trait]
impl<T: GitHubTransport> KnowledgeBackend for GitHubKnowledgeBackend<T> {
    async fn read_tree(&self, snapshot_id: &str) -> Result<GitHubTree, KnowledgeToolError> {
        let record = self
            .snapshots
            .get(snapshot_id)
            .ok_or_else(|| KnowledgeToolError::NotFound(snapshot_id.to_string()))?;

        self.client
            .read_tree(&record.repo.owner, &record.repo.name, &record.snapshot.rev)
            .await
    }

    async fn read_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<RepositoryManifest>, KnowledgeToolError> {
        let record = self
            .snapshots
            .get(snapshot_id)
            .ok_or_else(|| KnowledgeToolError::NotFound(snapshot_id.to_string()))?;

        let tree = self
            .client
            .read_tree(&record.repo.owner, &record.repo.name, &record.snapshot.rev)
            .await?;

        let manifest_paths = if record.repo.manifest_paths.is_empty() {
            DEFAULT_MANIFEST_PATHS
                .iter()
                .map(|path| path.to_string())
                .collect()
        } else {
            record.repo.manifest_paths.clone()
        };

        let Some(manifest_entry) = manifest_paths
            .iter()
            .flat_map(|manifest_path| {
                tree.entries
                    .iter()
                    .filter(move |entry| entry.path == *manifest_path)
            })
            .find(|entry| matches!(entry.kind, GitHubTreeEntryKind::Blob))
        else {
            return Ok(None);
        };

        let blob = self
            .client
            .read_blob(&record.repo.owner, &record.repo.name, &manifest_entry.sha)
            .await?;

        let value: Value = serde_json::from_str(&blob.text)
            .map_err(|err| KnowledgeToolError::manifest_parse(err.to_string()))?;
        let manifest = RepositoryManifest::from_json(value)?;
        Ok(Some(manifest))
    }

    async fn read_blob(
        &self,
        snapshot_id: &str,
        path: &str,
        sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError> {
        let record = self
            .snapshots
            .get(snapshot_id)
            .ok_or_else(|| KnowledgeToolError::NotFound(snapshot_id.to_string()))?;

        self.client
            .read_blob(&record.repo.owner, &record.repo.name, sha)
            .await
            .map(|blob| GitHubBlob {
                sha: blob.sha,
                text: blob.text,
            })
            .map_err(|err| match err {
                KnowledgeToolError::NotFound(_) => KnowledgeToolError::NotFound(path.to_string()),
                other => other,
            })
    }
}

fn check_api_error(value: &Value) -> Result<(), KnowledgeToolError> {
    let Some(message) = value.get("message").and_then(Value::as_str) else {
        return Ok(());
    };

    if message == "Not Found" {
        return Err(KnowledgeToolError::NotFound(message.to_string()));
    }

    if message.contains("API rate limit") {
        return Err(KnowledgeToolError::RateLimited(message.to_string()));
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GitHubRefResponse {
    object: GitHubRefObject,
}

#[derive(Debug, Deserialize)]
struct GitHubRefObject {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GitHubCommitResponse {
    tree: GitHubCommitTree,
}

#[derive(Debug, Deserialize)]
struct GitHubCommitTree {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GitHubTreeResponse {
    tree: Vec<GitHubTreeEntryResponse>,
}

#[derive(Debug, Deserialize)]
struct GitHubTreeEntryResponse {
    path: String,
    sha: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct GitHubBlobResponse {
    sha: String,
    content: String,
    encoding: String,
}
