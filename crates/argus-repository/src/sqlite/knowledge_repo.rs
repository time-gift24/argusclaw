//! SQLite implementation of KnowledgeRepoRepository and KnowledgeRepoProvider.

use argus_protocol::ids::AgentId;
use argus_protocol::{KnowledgeRepoProvider, KnowledgeRepoRecord};
use async_trait::async_trait;
use serde_json;

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;
use crate::traits::KnowledgeRepoRepository;
use crate::types::KnowledgeRepoRecord as DbKnowledgeRepoRecord;

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> Result<DbKnowledgeRepoRecord, DbError> {
    let manifest_paths: String = ArgusSqlite::get_column(row, "manifest_paths")?;
    let manifest_paths =
        serde_json::from_str(&manifest_paths).map_err(|e| DbError::QueryFailed {
            reason: format!("failed to decode knowledge repo manifest_paths: {e}"),
        })?;

    Ok(DbKnowledgeRepoRecord {
        id: ArgusSqlite::get_column(row, "id")?,
        repo: ArgusSqlite::get_column(row, "repo")?,
        repo_id: ArgusSqlite::get_column(row, "repo_id")?,
        provider: ArgusSqlite::get_column(row, "provider")?,
        owner: ArgusSqlite::get_column(row, "owner")?,
        name: ArgusSqlite::get_column(row, "name")?,
        default_branch: ArgusSqlite::get_column(row, "default_branch")?,
        manifest_paths,
        workspace: ArgusSqlite::get_column(row, "workspace")?,
    })
}

fn db_to_protocol(record: &DbKnowledgeRepoRecord) -> KnowledgeRepoRecord {
    KnowledgeRepoRecord {
        id: record.id,
        repo: record.repo.clone(),
        repo_id: record.repo_id.clone(),
        provider: record.provider.clone(),
        owner: record.owner.clone(),
        name: record.name.clone(),
        default_branch: record.default_branch.clone(),
        manifest_paths: record.manifest_paths.clone(),
        workspace: record.workspace.clone(),
    }
}

#[async_trait]
impl KnowledgeRepoRepository for ArgusSqlite {
    async fn upsert(&self, record: &DbKnowledgeRepoRecord) -> Result<i64, DbError> {
        let manifest_paths =
            serde_json::to_string(&record.manifest_paths).map_err(|e| DbError::QueryFailed {
                reason: format!(
                    "failed to encode manifest_paths for repo '{}': {e}",
                    record.repo_id
                ),
            })?;
        let result = sqlx::query(
            "INSERT INTO knowledge_repos (
                repo, repo_id, provider, owner, name, default_branch, manifest_paths, workspace
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repo_id) DO UPDATE SET
                repo = excluded.repo,
                provider = excluded.provider,
                owner = excluded.owner,
                name = excluded.name,
                default_branch = excluded.default_branch,
                manifest_paths = excluded.manifest_paths,
                workspace = excluded.workspace",
        )
        .bind(&record.repo)
        .bind(&record.repo_id)
        .bind(&record.provider)
        .bind(&record.owner)
        .bind(&record.name)
        .bind(&record.default_branch)
        .bind(manifest_paths)
        .bind(&record.workspace)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if result.last_insert_rowid() > 0 {
            Ok(result.last_insert_rowid())
        } else {
            let id: i64 = sqlx::query_scalar("SELECT id FROM knowledge_repos WHERE repo_id = ?")
                .bind(&record.repo_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
            Ok(id)
        }
    }

    async fn get(&self, id: i64) -> Result<Option<DbKnowledgeRepoRecord>, DbError> {
        let row = sqlx::query(
            "SELECT id, repo, repo_id, provider, owner, name, default_branch, manifest_paths, workspace
             FROM knowledge_repos WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| row_to_record(&r)).transpose()
    }

    async fn find_by_repo(&self, repo: &str) -> Result<Option<DbKnowledgeRepoRecord>, DbError> {
        let row = sqlx::query(
            "SELECT id, repo, repo_id, provider, owner, name, default_branch, manifest_paths, workspace
             FROM knowledge_repos
             WHERE repo = ? OR repo_id = ?
             ORDER BY id
             LIMIT 1",
        )
        .bind(repo)
        .bind(repo)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| row_to_record(&r)).transpose()
    }

    async fn list(&self) -> Result<Vec<DbKnowledgeRepoRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, repo, repo_id, provider, owner, name, default_branch, manifest_paths, workspace
             FROM knowledge_repos
             ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.iter().map(|r| row_to_record(r)).collect()
    }

    async fn delete(&self, id: i64) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM knowledge_repos WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_repos_for_agent(
        &self,
        agent_id: i64,
    ) -> Result<Vec<DbKnowledgeRepoRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT kr.id, kr.repo, kr.repo_id, kr.provider, kr.owner, kr.name,
                    kr.default_branch, kr.manifest_paths, kr.workspace
             FROM knowledge_repos kr
             INNER JOIN agent_knowledge_workspaces akw ON kr.workspace = akw.workspace
             WHERE akw.agent_id = ?
             ORDER BY kr.id",
        )
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|r| row_to_record(r)).collect()
    }

    async fn set_agent_workspaces(
        &self,
        agent_id: i64,
        workspaces: &[String],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query("DELETE FROM agent_knowledge_workspaces WHERE agent_id = ?")
            .bind(agent_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        for ws in workspaces {
            sqlx::query(
                "INSERT INTO agent_knowledge_workspaces (agent_id, workspace) VALUES (?, ?)",
            )
            .bind(agent_id)
            .bind(ws)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        }

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_agent_workspaces(&self, agent_id: i64) -> Result<Vec<String>, DbError> {
        let rows =
            sqlx::query("SELECT workspace FROM agent_knowledge_workspaces WHERE agent_id = ?")
                .bind(agent_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        rows.iter()
            .map(|r| -> Result<String, DbError> { ArgusSqlite::get_column(r, "workspace") })
            .collect()
    }
}

/// Implement the protocol-level `KnowledgeRepoProvider` for `ArgusSqlite`.
#[async_trait]
impl KnowledgeRepoProvider for ArgusSqlite {
    async fn list_repos(
        &self,
        agent_id: Option<AgentId>,
    ) -> Result<Vec<KnowledgeRepoRecord>, Box<dyn std::error::Error + Send + Sync>> {
        let records = if let Some(agent) = agent_id {
            <Self as KnowledgeRepoRepository>::list_repos_for_agent(self, agent.inner()).await?
        } else {
            <Self as KnowledgeRepoRepository>::list(self).await?
        };
        Ok(records.iter().map(db_to_protocol).collect())
    }

    async fn get_repo(
        &self,
        repo: &str,
        agent_id: Option<AgentId>,
    ) -> Result<KnowledgeRepoRecord, Box<dyn std::error::Error + Send + Sync>> {
        let record = <Self as KnowledgeRepoRepository>::find_by_repo(self, repo)
            .await?
            .ok_or_else(|| format!("repo not found: {repo}"))?;

        // If an agent is specified, validate the repo belongs to one of its workspaces
        if let Some(agent) = agent_id {
            let workspaces =
                <Self as KnowledgeRepoRepository>::list_agent_workspaces(self, agent.inner())
                    .await?;
            if !workspaces.is_empty() && !workspaces.contains(&record.workspace) {
                return Err(format!(
                    "repo '{repo}' (workspace '{}') not accessible by agent",
                    record.workspace
                )
                .into());
            }
        }

        Ok(db_to_protocol(&record))
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::sqlite::migrate;

    fn sample_record() -> DbKnowledgeRepoRecord {
        DbKnowledgeRepoRecord {
            id: 0,
            repo: "acme/docs".to_string(),
            repo_id: "acme-docs".to_string(),
            provider: "github".to_string(),
            owner: "acme".to_string(),
            name: "docs".to_string(),
            default_branch: "trunk".to_string(),
            manifest_paths: vec![
                "knowledge.json".to_string(),
                "docs/knowledge.json".to_string(),
            ],
            workspace: "payments".to_string(),
        }
    }

    #[tokio::test]
    async fn knowledge_repo_round_trips_full_descriptor_fields() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");
        migrate(&pool).await.expect("migrations should succeed");

        let sqlite = ArgusSqlite::new_with_key_material(pool, vec![7; 32]);
        let record = sample_record();

        let id = KnowledgeRepoRepository::upsert(&sqlite, &record)
            .await
            .expect("upsert should succeed");
        let stored = KnowledgeRepoRepository::get(&sqlite, id)
            .await
            .expect("lookup should succeed")
            .expect("record should exist");

        assert_eq!(stored, DbKnowledgeRepoRecord { id, ..record });
    }
}
