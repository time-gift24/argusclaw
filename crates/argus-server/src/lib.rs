//! Argus Server - Axum HTTP server for end-user chat.

pub mod auth;
pub mod config;
pub mod http;
pub mod routes;
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use argus_crypto::{Cipher, FileKeySource};
use argus_job::JobManager;
use argus_llm::ProviderManager;
use argus_mcp::{McpRuntime, McpRuntimeConfig, RmcpConnector};
use argus_protocol::{LlmProvider, LlmProviderId, ProviderId, ProviderResolver};
use argus_repository::postgres::{self, ArgusPostgres};
use argus_repository::traits::{
    AgentRepository, JobRepository, LlmProviderRepository, McpRepository, SessionRepository,
    ThreadRepository, UserRepository, UserSessionRepository,
};
use argus_session::{SessionManager, UserChatServices};
use argus_template::TemplateManager;
use argus_tool::{
    ApplyPatchTool, ChromeTool, GlobTool, GrepTool, HttpTool, ListDirTool, ReadTool, ShellTool,
    ToolManager, WriteFileTool,
};
use axum::Router;

use crate::auth::dev_oauth::DevOAuth2Provider;
use crate::auth::provider::OAuth2AuthProvider;
use crate::auth::session::AuthSession;
use crate::config::ServerConfig;
use crate::state::AppState;

struct ServerProviderResolver {
    provider_manager: Arc<ProviderManager>,
}

#[async_trait::async_trait]
impl ProviderResolver for ServerProviderResolver {
    async fn resolve(&self, id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
        self.provider_manager
            .get_provider(&LlmProviderId::new(id.inner()))
            .await
    }

    async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
        self.provider_manager.get_default_provider().await
    }

    async fn resolve_with_model(
        &self,
        id: ProviderId,
        model: &str,
    ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
        self.provider_manager
            .get_provider_with_model(&LlmProviderId::new(id.inner()), model)
            .await
    }
}

pub async fn build_state(config: ServerConfig) -> Result<AppState> {
    let pool = postgres::connect(&config.database_url)
        .await
        .context("connect postgres")?;
    postgres::migrate(&pool).await.context("migrate postgres")?;

    let repository = Arc::new(ArgusPostgres::new(pool));
    let cipher = Arc::new(Cipher::new(FileKeySource::from_env_or_default()));

    let llm_repository: Arc<dyn LlmProviderRepository> = repository.clone();
    let provider_manager = Arc::new(
        ProviderManager::new(llm_repository.clone())
            .with_credential_repo(repository.clone())
            .with_cipher(cipher),
    );
    let provider_resolver: Arc<dyn ProviderResolver> = Arc::new(ServerProviderResolver {
        provider_manager: provider_manager.clone(),
    });

    let template_manager = Arc::new(TemplateManager::new_repository_only(
        repository.clone() as Arc<dyn AgentRepository>
    ));
    let tool_manager = Arc::new(ToolManager::new());
    register_default_tools(&tool_manager);

    let trace_dir = default_trace_dir();
    std::fs::create_dir_all(&trace_dir).ok();

    let job_manager = Arc::new(JobManager::new_with_repositories(
        template_manager.clone(),
        provider_resolver.clone(),
        tool_manager.clone(),
        trace_dir.clone(),
        repository.clone() as Arc<dyn JobRepository>,
        repository.clone() as Arc<dyn ThreadRepository>,
        llm_repository.clone(),
    ));

    let mcp_runtime = Arc::new(McpRuntime::new(
        repository.clone() as Arc<dyn McpRepository>,
        Arc::new(RmcpConnector),
        McpRuntimeConfig::default(),
    ));
    McpRuntime::start(&mcp_runtime);
    let mcp_tool_resolver: Arc<dyn argus_protocol::McpToolResolver> =
        Arc::new(McpRuntime::handle(&mcp_runtime));

    let session_manager = Arc::new(SessionManager::new(
        repository.clone() as Arc<dyn SessionRepository>,
        repository.clone() as Arc<dyn ThreadRepository>,
        llm_repository,
        template_manager,
        provider_resolver,
        mcp_tool_resolver,
        tool_manager,
        trace_dir,
        job_manager.thread_pool(),
        job_manager,
    ));

    let chat_services = Arc::new(UserChatServices::new(
        session_manager,
        repository.clone() as Arc<dyn UserSessionRepository>,
    ));

    Ok(AppState {
        config: Arc::new(config.clone()),
        auth_provider: Arc::new(DevOAuth2Provider::new()) as Arc<dyn OAuth2AuthProvider>,
        user_repo: repository as Arc<dyn UserRepository>,
        auth_session: Arc::new(AuthSession::new(&config.session_secret)),
        chat_services,
    })
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(auth::routes::router())
        .merge(routes::router())
        .with_state(state)
}

fn register_default_tools(tool_manager: &Arc<ToolManager>) {
    tool_manager.register(Arc::new(ShellTool::new()));
    tool_manager.register(Arc::new(ReadTool::new()));
    tool_manager.register(Arc::new(GrepTool::new()));
    tool_manager.register(Arc::new(GlobTool::new()));
    tool_manager.register(Arc::new(HttpTool::new()));
    tool_manager.register(Arc::new(WriteFileTool::new()));
    tool_manager.register(Arc::new(ListDirTool::new()));
    tool_manager.register(Arc::new(ApplyPatchTool::new()));
    tool_manager.register(Arc::new(ChromeTool::new_interactive()));
}

fn default_trace_dir() -> PathBuf {
    std::env::var("TRACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".arguswing")
                .join("traces")
        })
}
