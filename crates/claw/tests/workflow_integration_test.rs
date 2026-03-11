//! Integration tests for the workflow module.
//!
//! These tests verify end-to-end workflow management including:
//! - Workflow, stage, and job creation
//! - Status updates with timestamps
//! - Cascade deletion of stages and jobs
//! - Foreign key constraints

use uuid::Uuid;

use claw::agents::AgentId;
use claw::db::sqlite::{connect, migrate};
use claw::db::workflow::SqliteWorkflowRepository;
use claw::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowRepository,
    WorkflowStatus,
};

/// Creates a test database pool and runs migrations.
async fn create_test_pool() -> sqlx::SqlitePool {
    let pool = connect("sqlite::memory:").await.expect("failed to connect");
    migrate(&pool).await.expect("failed to migrate");
    pool
}

/// Creates a test LLM provider for foreign key dependencies.
async fn create_test_provider(pool: &sqlx::SqlitePool, id: &str) {
    sqlx::query(
        "INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(id)
    .bind("openai_compatible")
    .bind(format!("Test Provider {id}"))
    .bind("https://api.example.com")
    .bind("gpt-4")
    .bind(vec![1, 2, 3, 4]) // Dummy encrypted key
    .bind(vec![5, 6, 7, 8]) // Dummy nonce
    .execute(pool)
    .await
    .expect("failed to create test provider");
}

/// Creates a test agent for foreign key dependencies.
async fn create_test_agent(pool: &sqlx::SqlitePool, agent_id: &str, provider_id: &str) {
    sqlx::query(
        "INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(agent_id)
    .bind(format!("Agent {agent_id}"))
    .bind("Test agent")
    .bind("1.0.0")
    .bind(provider_id)
    .bind("You are a test agent")
    .bind("[]")
    .bind(1000_i64)
    .bind(70_i64) // Temperature as integer (0.7 * 100)
    .execute(pool)
    .await
    .expect("failed to create test agent");
}

#[tokio::test]
async fn full_workflow_lifecycle() {
    let pool = create_test_pool().await;
    let repo = SqliteWorkflowRepository::new(pool.clone());

    // Generate unique IDs using UUID
    let workflow_id = Uuid::new_v4().to_string();
    let stage1_id = Uuid::new_v4().to_string();
    let stage2_id = Uuid::new_v4().to_string();
    let job1_id = Uuid::new_v4().to_string();
    let job2_id = Uuid::new_v4().to_string();
    let agent_id = Uuid::new_v4().to_string();
    let provider_id = Uuid::new_v4().to_string();

    // Step 1: Create a workflow
    let workflow = WorkflowRecord {
        id: WorkflowId::new(workflow_id.clone()),
        name: "Integration Test Workflow".to_string(),
        status: WorkflowStatus::Pending,
    };
    repo.create_workflow(&workflow).await.unwrap();

    let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, workflow.id);
    assert_eq!(retrieved.name, workflow.name);
    assert_eq!(retrieved.status, WorkflowStatus::Pending);

    // Step 2: Create stages
    let stage1 = StageRecord {
        id: StageId::new(stage1_id.clone()),
        workflow_id: WorkflowId::new(workflow_id.clone()),
        name: "Setup Stage".to_string(),
        sequence: 1,
        status: WorkflowStatus::Pending,
    };
    let stage2 = StageRecord {
        id: StageId::new(stage2_id.clone()),
        workflow_id: WorkflowId::new(workflow_id.clone()),
        name: "Execution Stage".to_string(),
        sequence: 2,
        status: WorkflowStatus::Pending,
    };

    repo.create_stage(&stage1).await.unwrap();
    repo.create_stage(&stage2).await.unwrap();

    let stages = repo.list_stages_by_workflow(&workflow.id).await.unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].sequence, 1);
    assert_eq!(stages[0].name, "Setup Stage");
    assert_eq!(stages[1].sequence, 2);
    assert_eq!(stages[1].name, "Execution Stage");

    // Step 3: Create provider and agent for job foreign key
    create_test_provider(&pool, &provider_id).await;
    create_test_agent(&pool, &agent_id, &provider_id).await;

    // Step 4: Create jobs
    let job1 = JobRecord {
        id: JobId::new(job1_id.clone()),
        stage_id: StageId::new(stage1_id.clone()),
        agent_id: AgentId::new(agent_id.clone()),
        name: "Setup Job A".to_string(),
        status: WorkflowStatus::Pending,
        started_at: None,
        finished_at: None,
    };
    let job2 = JobRecord {
        id: JobId::new(job2_id.clone()),
        stage_id: StageId::new(stage1_id.clone()),
        agent_id: AgentId::new(agent_id.clone()),
        name: "Setup Job B".to_string(),
        status: WorkflowStatus::Pending,
        started_at: None,
        finished_at: None,
    };

    repo.create_job(&job1).await.unwrap();
    repo.create_job(&job2).await.unwrap();

    let jobs = repo.list_jobs_by_stage(&stage1.id).await.unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].name, "Setup Job A");
    assert_eq!(jobs[1].name, "Setup Job B");

    // Step 5: Update job status with timestamps
    repo.update_job_status(
        &job1.id,
        WorkflowStatus::Running,
        Some("2024-03-11T10:00:00Z"),
        None,
    )
    .await
    .unwrap();

    let jobs = repo.list_jobs_by_stage(&stage1.id).await.unwrap();
    assert_eq!(jobs[0].status, WorkflowStatus::Running);
    assert_eq!(jobs[0].started_at, Some("2024-03-11T10:00:00Z".to_string()));
    assert!(jobs[0].finished_at.is_none());

    repo.update_job_status(
        &job1.id,
        WorkflowStatus::Succeeded,
        Some("2024-03-11T10:00:00Z"),
        Some("2024-03-11T10:05:00Z"),
    )
    .await
    .unwrap();

    let jobs = repo.list_jobs_by_stage(&stage1.id).await.unwrap();
    assert_eq!(jobs[0].status, WorkflowStatus::Succeeded);
    assert_eq!(jobs[0].started_at, Some("2024-03-11T10:00:00Z".to_string()));
    assert_eq!(
        jobs[0].finished_at,
        Some("2024-03-11T10:05:00Z".to_string())
    );

    // Step 6: Update workflow status
    repo.update_workflow_status(&workflow.id, WorkflowStatus::Running)
        .await
        .unwrap();

    let retrieved = repo.get_workflow(&workflow.id).await.unwrap().unwrap();
    assert_eq!(retrieved.status, WorkflowStatus::Running);

    // Step 7: Cascade delete - deleting workflow should delete stages and jobs
    let deleted = repo.delete_workflow(&workflow.id).await.unwrap();
    assert!(deleted);

    // Verify workflow is deleted
    let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
    assert!(retrieved.is_none());

    // Verify stages are cascade deleted
    let stages = repo.list_stages_by_workflow(&workflow.id).await.unwrap();
    assert_eq!(stages.len(), 0);

    // Verify jobs are cascade deleted
    let jobs = repo.list_jobs_by_stage(&stage1.id).await.unwrap();
    assert_eq!(jobs.len(), 0);
}

#[tokio::test]
async fn foreign_key_constraint_agent() {
    let pool = create_test_pool().await;
    let repo = SqliteWorkflowRepository::new(pool.clone());

    // Generate unique IDs using UUID
    let workflow_id = Uuid::new_v4().to_string();
    let stage_id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let non_existent_agent_id = Uuid::new_v4().to_string();

    // Create workflow and stage
    let workflow = WorkflowRecord {
        id: WorkflowId::new(workflow_id.clone()),
        name: "FK Test Workflow".to_string(),
        status: WorkflowStatus::Pending,
    };
    repo.create_workflow(&workflow).await.unwrap();

    let stage = StageRecord {
        id: StageId::new(stage_id.clone()),
        workflow_id: WorkflowId::new(workflow_id.clone()),
        name: "FK Test Stage".to_string(),
        sequence: 1,
        status: WorkflowStatus::Pending,
    };
    repo.create_stage(&stage).await.unwrap();

    // Attempt to create a job with a non-existent agent_id
    // This should fail due to foreign key constraint
    let job = JobRecord {
        id: JobId::new(job_id.clone()),
        stage_id: StageId::new(stage_id.clone()),
        agent_id: AgentId::new(non_existent_agent_id.clone()),
        name: "FK Test Job".to_string(),
        status: WorkflowStatus::Pending,
        started_at: None,
        finished_at: None,
    };

    let result = repo.create_job(&job).await;

    // Verify that the operation failed
    assert!(result.is_err());

    let err = result.unwrap_err();
    // The error should be a QueryFailed variant (foreign key constraint violation)
    match err {
        claw::db::DbError::QueryFailed { reason } => {
            // SQLite foreign key constraint error messages typically contain
            // "FOREIGN KEY constraint failed"
            assert!(
                reason.contains("FOREIGN KEY") || reason.contains("foreign key"),
                "Expected foreign key error, got: {reason}"
            );
        }
        _ => {
            panic!("Expected DbError::QueryFailed for foreign key violation, got: {err:?}");
        }
    }

    // Verify that the job was not created
    let jobs = repo.list_jobs_by_stage(&stage.id).await.unwrap();
    assert_eq!(jobs.len(), 0);
}
