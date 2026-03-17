#![cfg(feature = "dev")]

use std::fs;
use std::path::Path;

use claw::{DEFAULT_AGENT_ID, connect};
use sqlx::migrate::Migrator;
use sqlx::{Row, SqlitePool};

const OLD_MIGRATION_FILES: &[&str] = &[
    "20260312050411_init.sql",
    "20260312050412_create_threads.sql",
    "20260312050413_generalize_jobs.sql",
    "20260312050414_create_users_table.sql",
    "20260312050415_multi_model_provider.sql",
    "20260317090000_add_provider_model_config.sql",
];

const ALL_MIGRATION_FILES: &[&str] = &[
    "20260312050411_init.sql",
    "20260312050412_create_threads.sql",
    "20260312050413_generalize_jobs.sql",
    "20260312050414_create_users_table.sql",
    "20260312050415_multi_model_provider.sql",
    "20260317090000_add_provider_model_config.sql",
    "20260317101000_generated_provider_ids.sql",
    "20260317102000_generated_agent_ids.sql",
    "20260317123000_add_agent_model.sql",
];

fn stage_migrations(target_dir: &Path, file_names: &[&str]) {
    let source_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    fs::create_dir_all(target_dir).expect("migration directory should be created");

    for file_name in file_names {
        fs::copy(source_dir.join(file_name), target_dir.join(file_name))
            .unwrap_or_else(|error| panic!("failed to stage migration {file_name}: {error}"));
    }
}

async fn run_migrations(pool: &SqlitePool, dir: &Path) {
    let migrator = Migrator::new(dir)
        .await
        .unwrap_or_else(|error| panic!("failed to load migrator from {}: {error}", dir.display()));
    migrator
        .run(pool)
        .await
        .unwrap_or_else(|error| panic!("failed to run migrations from {}: {error}", dir.display()));
}

fn is_generated_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[tokio::test]
async fn generated_id_migrations_rewrite_existing_provider_and_agent_references() {
    let temp_dir = tempfile::tempdir().expect("temp dir should exist");
    let old_migrations_dir = temp_dir.path().join("migrations-old");
    let full_migrations_dir = temp_dir.path().join("migrations-full");
    let database_path = temp_dir.path().join("generated-ids.db");

    stage_migrations(&old_migrations_dir, OLD_MIGRATION_FILES);
    stage_migrations(&full_migrations_dir, ALL_MIGRATION_FILES);

    let pool = connect(
        database_path
            .to_str()
            .expect("database path should be valid utf-8"),
    )
    .await
    .expect("sqlite pool should connect");

    run_migrations(&pool, &old_migrations_dir).await;

    let old_provider_id = "legacy/provider 1";
    let old_agent_id = "legacy agent/1";

    sqlx::query(
        r#"
        INSERT INTO llm_providers (
            id, kind, display_name, base_url, models, default_model,
            encrypted_api_key, api_key_nonce, extra_headers, is_default, model_config
        )
        VALUES (?1, 'openai-compatible', 'Legacy Provider', 'https://legacy.example.com/v1', '["gpt-4.1"]', 'gpt-4.1', X'00', X'00', '{}', 1, '{"gpt-4.1":{"context_length":128000}}')
        "#,
    )
    .bind(old_provider_id)
    .execute(&pool)
    .await
    .expect("legacy provider should be inserted");

    sqlx::query(
        r#"
        INSERT INTO agents (
            id, display_name, description, version, provider_id, system_prompt, tool_names
        )
        VALUES (?1, 'Legacy Agent', 'Migrated test agent', '1.0.0', ?2, 'You are a migrated agent.', '["shell"]')
        "#,
    )
    .bind(old_agent_id)
    .bind(old_provider_id)
    .execute(&pool)
    .await
    .expect("legacy agent should be inserted");

    sqlx::query(
        r#"
        INSERT INTO agents (
            id, display_name, description, version, system_prompt, tool_names
        )
        VALUES (?1, 'ArgusWing', 'Built-in agent', '1.0.0', 'You are ArgusWing.', '["shell","read"]')
        "#,
    )
    .bind(DEFAULT_AGENT_ID)
    .execute(&pool)
    .await
    .expect("default built-in agent should be inserted");

    sqlx::query("INSERT INTO threads (id, provider_id, title) VALUES (?1, ?2, 'Legacy Thread')")
        .bind("thread-1")
        .bind(old_provider_id)
        .execute(&pool)
        .await
        .expect("legacy thread should be inserted");

    sqlx::query(
        r#"
        INSERT INTO jobs (id, job_type, name, status, agent_id, prompt, depends_on)
        VALUES ('job-1', 'standalone', 'Legacy Job', 'pending', ?1, 'Do legacy work', '[]')
        "#,
    )
    .bind(old_agent_id)
    .execute(&pool)
    .await
    .expect("legacy job should be inserted");

    run_migrations(&pool, &full_migrations_dir).await;

    let migrated_provider_id: String =
        sqlx::query_scalar("SELECT id FROM llm_providers WHERE display_name = 'Legacy Provider'")
            .fetch_one(&pool)
            .await
            .expect("migrated provider should exist");
    assert_ne!(migrated_provider_id, old_provider_id);
    assert!(is_generated_id(&migrated_provider_id));

    let migrated_agent_row =
        sqlx::query("SELECT id, provider_id FROM agents WHERE display_name = 'Legacy Agent'")
            .fetch_one(&pool)
            .await
            .expect("migrated agent should exist");
    let migrated_agent_id: String = migrated_agent_row
        .try_get("id")
        .expect("migrated agent id should be readable");
    let migrated_agent_provider_id: String = migrated_agent_row
        .try_get("provider_id")
        .expect("migrated agent provider id should be readable");

    assert_ne!(migrated_agent_id, old_agent_id);
    assert!(is_generated_id(&migrated_agent_id));
    assert_eq!(migrated_agent_provider_id, migrated_provider_id);

    let migrated_thread_provider_id: String =
        sqlx::query_scalar("SELECT provider_id FROM threads WHERE id = 'thread-1'")
            .fetch_one(&pool)
            .await
            .expect("migrated thread should exist");
    assert_eq!(migrated_thread_provider_id, migrated_provider_id);

    let migrated_job_agent_id: String =
        sqlx::query_scalar("SELECT agent_id FROM jobs WHERE id = 'job-1'")
            .fetch_one(&pool)
            .await
            .expect("migrated job should exist");
    assert_eq!(migrated_job_agent_id, migrated_agent_id);

    let built_in_agent_id: String =
        sqlx::query_scalar("SELECT id FROM agents WHERE display_name = 'ArgusWing'")
            .fetch_one(&pool)
            .await
            .expect("built-in agent should still exist");
    assert_eq!(built_in_agent_id, DEFAULT_AGENT_ID);
}
