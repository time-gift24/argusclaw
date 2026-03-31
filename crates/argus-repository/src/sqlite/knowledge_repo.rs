//! SQLite implementation of KnowledgeRepoRepository and KnowledgeRepoProvider.

use async_trait::async_trait;
use argus_protocol::ids::AgentId;
use argus_protocol::{KnowledgeRepoProvider, KnowledgeRepoRecord};

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;
use crate::traits::KnowledgeRepoRepository;
use crate::types::KnowledgeRepoRecord as DbKnowledgeRepoRecord;

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> Result<DbKnowledgeRepoRecord, DbError> {
    Ok(DbKnowledgeRepoRecord {
        id: ArgusSqlite::get_column(row, "id")?,
        repo: ArgusSqlite::get_column(row, "repo")?,
        workspace: ArgusSqlite::get_column(row, "workspace")?,
    })
}

fn db_to_protocol(record: &DbKnowledgeRepoRecord) -> KnowledgeRepoRecord {
    KnowledgeRepoRecord {
        id: record.id,
        repo: record.repo.clone(),
        workspace: record.workspace.clone(),
    }
}

#[async_trait]
impl KnowledgeRepoRepository for ArgusSqlite {
    async fn upsert(&self, repo: &str, workspace: &str) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO knowledge_repos (repo, workspace) VALUES (?, ?)
             ON CONFLICT(repo) DO UPDATE SET workspace = excluded.workspace",
        )
        .bind(repo)
        .bind(workspace)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        if result.last_insert_rowid() > 0 {
            Ok(result.last_insert_rowid())
        } else {
            let id: i64 =
                sqlx::query_scalar("SELECT id FROM knowledge_repos WHERE repo = ?")
                    .bind(repo)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
            Ok(id)
        }
    }

    async fn get(&self, id: i64) -> Result<Option<DbKnowledgeRepoRecord>, DbError> {
        let row = sqlx::query("SELECT id, repo, workspace FROM knowledge_repos WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| row_to_record(&r)).transpose()
    }

    async fn find_by_repo(&self, repo: &str) -> Result<Option<DbKnowledgeRepoRecord>, DbError> {
        let row = sqlx::query("SELECT id, repo, workspace FROM knowledge_repos WHERE repo = ?")
            .bind(repo)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| row_to_record(&r)).transpose()
    }

    async fn list(&self) -> Result<Vec<DbKnowledgeRepoRecord>, DbError> {
        let rows =
            sqlx::query("SELECT id, repo, workspace FROM knowledge_repos ORDER BY id")
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
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_repos_for_agent(
        &self,
        agent_id: i64,
    ) -> Result<Vec<DbKnowledgeRepoRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT kr.id, kr.repo, kr.workspace
             FROM knowledge_repos kr
             INNER JOIN agent_knowledge_workspaces akw ON kr.workspace = akw.workspace
             WHERE akw.agent_id = ?
             ORDER BY kr.id",
        )
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

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
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        for ws in workspaces {
            sqlx::query(
                "INSERT INTO agent_knowledge_workspaces (agent_id, workspace) VALUES (?, ?)",
            )
            .bind(agent_id)
            .bind(ws)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
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
                .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

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
