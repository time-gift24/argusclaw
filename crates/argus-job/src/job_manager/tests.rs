use std::sync::Arc;

use argus_agent::thread_trace_store::{
    ThreadTraceKind, ThreadTraceMetadata, chat_thread_base_dir, persist_thread_metadata,
};
use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProviderRepository,
};
use argus_protocol::{
    AgentRecord, LlmProvider, ProviderId, SessionId, ThinkingConfig, ThreadId, ThreadRuntimeStatus,
};
use argus_repository::ArgusSqlite;
use argus_repository::migrate;
use argus_repository::traits::{
    AgentRepository, JobRepository, SessionRepository, ThreadRepository,
};
use argus_repository::types::{AgentId as RepoAgentId, ThreadRecord};
use argus_template::TemplateManager;
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::SqlitePool;

use argus_protocol::TokenUsage;
use argus_tool::ToolManager;
use tokio::time::{Duration, sleep, timeout};

use super::*;

mod cancellation;
mod execution;
mod recovery;
mod summary;
mod support;
mod tracking;

use support::*;
