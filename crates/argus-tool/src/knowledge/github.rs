use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

use super::error::KnowledgeToolError;
use super::models::{GitHubBlob, GitHubSnapshot, GitHubTree, GitHubTreeEntry, GitHubTreeEntryKind};

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
        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/git/ref/heads/{ref_name}"
        );
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
        let url = format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{rev}");
        let value = self.transport.get_json(&url).await?;
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
