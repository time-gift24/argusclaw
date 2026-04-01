use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, Tool as RmcpTool};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, serve_client};
use serde::{Deserialize, Serialize};

use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentId, ArgusError, McpDiscoveredToolRecord, McpServerRecord, McpServerStatus,
    McpToolResolver, McpTransportConfig, ResolvedMcpTools,
};
pub use argus_repository::traits::McpRepository;

use crate::error::McpRuntimeError;
use crate::supervisor::{retry_delay, should_poll_server, spawn_supervisor};
use crate::tool_adapter::{McpToolAdapter, McpToolExecutor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConnectionTestResult {
    pub status: McpServerStatus,
    pub checked_at: String,
    pub latency_ms: u64,
    pub discovered_tools: Vec<McpDiscoveredToolRecord>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct McpRuntimeConfig {
    pub supervisor_poll_interval: Duration,
    pub ready_recheck_interval: Duration,
    pub initial_retry_delay: Duration,
    pub max_retry_delay: Duration,
}

impl Default for McpRuntimeConfig {
    fn default() -> Self {
        Self {
            supervisor_poll_interval: Duration::from_secs(30),
            ready_recheck_interval: Duration::from_secs(300),
            initial_retry_delay: Duration::from_secs(5),
            max_retry_delay: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRuntimeSnapshot {
    pub server_id: i64,
    pub display_name: String,
    pub status: McpServerStatus,
    pub retry_attempts: u32,
    pub next_retry_delay: Option<Duration>,
    pub last_error: Option<String>,
    pub discovered_tool_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpRuntimeSnapshot {
    pub servers: Vec<McpServerRuntimeSnapshot>,
}

#[async_trait]
pub trait McpSession: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<McpDiscoveredToolRecord>, McpRuntimeError>;

    async fn call_tool(
        &self,
        tool_name_original: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError>;
}

#[async_trait]
pub trait McpConnector: Send + Sync {
    async fn connect(
        &self,
        server: &McpServerRecord,
    ) -> Result<Arc<dyn McpSession>, McpRuntimeError>;
}

struct ServerRuntimeState {
    record: McpServerRecord,
    tools: Vec<McpDiscoveredToolRecord>,
    session: Option<Arc<dyn McpSession>>,
    retry_attempts: u32,
    next_retry_at: Option<Instant>,
    last_checked_instant: Option<Instant>,
    urgent_retry: bool,
}

impl ServerRuntimeState {
    fn new(record: McpServerRecord, tools: Vec<McpDiscoveredToolRecord>) -> Self {
        Self {
            record,
            tools,
            session: None,
            retry_attempts: 0,
            next_retry_at: None,
            last_checked_instant: None,
            urgent_retry: false,
        }
    }

    fn snapshot(&self) -> Option<McpServerRuntimeSnapshot> {
        let server_id = self.record.id?;
        let next_retry_delay = self.next_retry_at.map(|deadline| {
            let now = Instant::now();
            if deadline > now {
                deadline.duration_since(now)
            } else {
                Duration::ZERO
            }
        });

        Some(McpServerRuntimeSnapshot {
            server_id,
            display_name: self.record.display_name.clone(),
            status: self.record.status,
            retry_attempts: self.retry_attempts,
            next_retry_delay,
            last_error: self.record.last_error.clone(),
            discovered_tool_count: self.record.discovered_tool_count,
        })
    }
}

#[derive(Default)]
struct RuntimeState {
    servers: HashMap<i64, ServerRuntimeState>,
}

pub struct McpRuntime {
    repo: Arc<dyn McpRepository>,
    connector: Arc<dyn McpConnector>,
    config: McpRuntimeConfig,
    state: Mutex<RuntimeState>,
    supervisor_started: AtomicBool,
    supervisor_wakeup: tokio::sync::Notify,
}

#[derive(Clone)]
pub struct McpRuntimeHandle {
    inner: Arc<McpRuntime>,
}

impl McpRuntimeHandle {
    #[must_use]
    pub fn new(inner: Arc<McpRuntime>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn inner(&self) -> &Arc<McpRuntime> {
        &self.inner
    }

    pub fn start(&self) {
        McpRuntime::start(&self.inner);
    }

    pub async fn resolve_for_agent(
        &self,
        agent_id: AgentId,
    ) -> Result<ResolvedMcpTools, McpRuntimeError> {
        let executor: Arc<dyn McpToolExecutor> = Arc::new(self.clone());
        self.inner
            .resolve_for_agent_with_executor(agent_id, executor)
            .await
    }
}

impl Deref for McpRuntimeHandle {
    type Target = McpRuntime;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

#[async_trait]
impl McpToolResolver for McpRuntimeHandle {
    async fn resolve_for_agent(
        &self,
        agent_id: AgentId,
    ) -> argus_protocol::Result<ResolvedMcpTools> {
        self.resolve_for_agent(agent_id)
            .await
            .map_err(ArgusError::from)
    }
}

#[async_trait]
impl McpToolExecutor for McpRuntimeHandle {
    async fn execute_mcp_tool(
        &self,
        server_id: i64,
        tool_name_original: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        self.inner
            .call_tool(server_id, tool_name_original, input)
            .await
    }
}

impl McpRuntime {
    #[must_use]
    pub fn new(
        repo: Arc<dyn McpRepository>,
        connector: Arc<dyn McpConnector>,
        config: McpRuntimeConfig,
    ) -> Self {
        Self {
            repo,
            connector,
            config,
            state: Mutex::new(RuntimeState::default()),
            supervisor_started: AtomicBool::new(false),
            supervisor_wakeup: tokio::sync::Notify::new(),
        }
    }

    #[must_use]
    pub fn handle(runtime: &Arc<Self>) -> McpRuntimeHandle {
        McpRuntimeHandle::new(Arc::clone(runtime))
    }

    #[must_use]
    pub fn config(&self) -> &McpRuntimeConfig {
        &self.config
    }

    pub fn start(runtime: &Arc<Self>) {
        if runtime
            .supervisor_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            spawn_supervisor(Arc::clone(runtime));
        }
    }

    pub fn snapshot(&self) -> McpRuntimeSnapshot {
        let mut servers = self
            .state_guard()
            .servers
            .values()
            .filter_map(ServerRuntimeState::snapshot)
            .collect::<Vec<_>>();
        servers.sort_by_key(|server| server.server_id);
        McpRuntimeSnapshot { servers }
    }

    pub fn server_snapshot(&self, server_id: i64) -> Option<McpServerRuntimeSnapshot> {
        self.state_guard()
            .servers
            .get(&server_id)
            .and_then(ServerRuntimeState::snapshot)
    }

    pub async fn test_server_input(
        &self,
        record: McpServerRecord,
    ) -> Result<McpConnectionTestResult, McpRuntimeError> {
        let started = Instant::now();
        let checked_at = Utc::now().to_rfc3339();
        let server_id = record.id.unwrap_or_default();
        let failed_result = |message: String| McpConnectionTestResult {
            status: McpServerStatus::Failed,
            checked_at: checked_at.clone(),
            latency_ms: started.elapsed().as_millis() as u64,
            discovered_tools: Vec::new(),
            message,
        };

        match timeout_operation(
            record.timeout_ms,
            self.connector.connect(&record),
            || McpRuntimeError::ConnectFailed {
                server_id,
                reason: format!("connection timed out after {}ms", record.timeout_ms),
            },
            |error| error,
        )
        .await
        {
            Ok(session) => {
                match timeout_operation(
                    record.timeout_ms,
                    session.list_tools(),
                    || McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: format!("tool discovery timed out after {}ms", record.timeout_ms),
                    },
                    |error| error,
                )
                .await
                {
                    Ok(tools) => Ok(McpConnectionTestResult {
                        status: McpServerStatus::Ready,
                        checked_at,
                        latency_ms: started.elapsed().as_millis() as u64,
                        discovered_tools: normalize_tools(server_id, tools),
                        message: "connection succeeded".to_string(),
                    }),
                    Err(error) => Ok(failed_result(error.to_string())),
                }
            }
            Err(error) => Ok(failed_result(error.to_string())),
        }
    }

    pub async fn poll_once(&self) -> Result<(), McpRuntimeError> {
        self.load_servers_from_repo().await?;
        let due_server_ids = self.due_server_ids();

        for server_id in due_server_ids {
            self.refresh_server(server_id).await?;
        }

        Ok(())
    }

    pub async fn call_tool(
        &self,
        server_id: i64,
        tool_name_original: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let (session, timeout_ms) = {
            let state = self.state_guard();
            let entry = state
                .servers
                .get(&server_id)
                .ok_or(McpRuntimeError::ServerNotFound { server_id })?;
            if entry.record.status != McpServerStatus::Ready {
                return Err(McpRuntimeError::ServerNotReady { server_id });
            }
            (
                entry
                    .session
                    .clone()
                    .ok_or(McpRuntimeError::ServerNotReady { server_id })?,
                entry.record.timeout_ms,
            )
        };

        let result = timeout_operation(
            timeout_ms,
            session.call_tool(tool_name_original, input),
            || McpRuntimeError::ConnectFailed {
                server_id,
                reason: format!("tool call timed out after {}ms", timeout_ms),
            },
            |error| error,
        )
        .await;

        if let Err(McpRuntimeError::ConnectFailed { reason, .. }) = &result {
            self.record_runtime_disconnect(server_id, reason.clone())
                .await;
        }

        result
    }

    pub async fn ensure_retry_scheduled(&self, server_id: i64) -> Result<(), McpRuntimeError> {
        let scheduled_at = Instant::now() + self.config.initial_retry_delay;
        let mut state = self.state_guard();
        let entry = state
            .servers
            .get_mut(&server_id)
            .ok_or(McpRuntimeError::ServerNotFound { server_id })?;

        if !entry.record.enabled || entry.record.status == McpServerStatus::Disabled {
            return Ok(());
        }

        if entry.record.status == McpServerStatus::Ready && entry.session.is_some() {
            return Ok(());
        }

        entry.record.status = McpServerStatus::Retrying;
        entry.urgent_retry = true;
        match entry.next_retry_at {
            Some(existing_deadline) if existing_deadline <= scheduled_at => {}
            _ => {
                entry.next_retry_at = Some(scheduled_at);
            }
        }
        self.notify_supervisor();

        Ok(())
    }

    async fn resolve_for_agent_with_executor(
        &self,
        agent_id: AgentId,
        executor: Arc<dyn McpToolExecutor>,
    ) -> Result<ResolvedMcpTools, McpRuntimeError> {
        self.load_servers_from_repo().await?;
        let bindings = self.repo.list_agent_mcp_bindings(agent_id).await?;

        let mut resolved_tools: Vec<Arc<dyn NamedTool>> = Vec::new();
        let mut unavailable_servers = Vec::new();
        let mut retry_server_ids = Vec::new();

        {
            let mut state = self.state_guard();
            for binding in bindings {
                let Some(entry) = state.servers.get_mut(&binding.server.server_id) else {
                    unavailable_servers.push(argus_protocol::McpUnavailableServerSummary {
                        server_id: binding.server.server_id,
                        display_name: format!("MCP server {}", binding.server.server_id),
                        reason: "server not loaded".to_string(),
                    });
                    continue;
                };

                if !entry.record.enabled || entry.record.status == McpServerStatus::Disabled {
                    unavailable_servers.push(argus_protocol::McpUnavailableServerSummary {
                        server_id: binding.server.server_id,
                        display_name: entry.record.display_name.clone(),
                        reason: "server disabled".to_string(),
                    });
                    continue;
                }

                if entry.record.status != McpServerStatus::Ready || entry.session.is_none() {
                    retry_server_ids.push(binding.server.server_id);
                    unavailable_servers.push(argus_protocol::McpUnavailableServerSummary {
                        server_id: binding.server.server_id,
                        display_name: entry.record.display_name.clone(),
                        reason: entry
                            .record
                            .last_error
                            .clone()
                            .unwrap_or_else(|| "server not ready".to_string()),
                    });
                    continue;
                }

                for tool in entry
                    .tools
                    .iter()
                    .filter(|tool| binding.allows_tool(&tool.tool_name_original))
                {
                    resolved_tools.push(Arc::new(McpToolAdapter::new(
                        Arc::clone(&executor),
                        &entry.record.display_name,
                        tool,
                    )));
                }
            }
        }

        for server_id in retry_server_ids {
            self.ensure_retry_scheduled(server_id).await?;
        }

        Ok(ResolvedMcpTools::new(resolved_tools, unavailable_servers))
    }

    async fn load_servers_from_repo(&self) -> Result<(), McpRuntimeError> {
        let servers = self.repo.list_mcp_servers().await?;
        let mut loaded = Vec::with_capacity(servers.len());
        for record in servers {
            let server_id = record
                .id
                .ok_or_else(|| McpRuntimeError::InvalidConfiguration {
                    display_name: record.display_name.clone(),
                    reason: "server record is missing a persisted id".to_string(),
                })?;
            let tools = self.repo.list_mcp_server_tools(server_id).await?;
            loaded.push((server_id, record, tools));
        }

        let loaded_ids = loaded
            .iter()
            .map(|(server_id, ..)| *server_id)
            .collect::<HashSet<_>>();
        let mut state = self.state_guard();
        state
            .servers
            .retain(|server_id, _| loaded_ids.contains(server_id));

        for (server_id, mut record, tools) in loaded {
            if !record.enabled {
                record.status = McpServerStatus::Disabled;
            }

            match state.servers.get_mut(&server_id) {
                Some(entry) => {
                    let transport_changed = entry.record.transport != record.transport
                        || entry.record.timeout_ms != record.timeout_ms
                        || entry.record.enabled != record.enabled;
                    entry.tools = tools;
                    entry.record = record;
                    if entry.session.is_some() {
                        entry.record.status = McpServerStatus::Ready;
                    } else if !entry.record.enabled {
                        entry.record.status = McpServerStatus::Disabled;
                    } else if entry.next_retry_at.is_some() || entry.urgent_retry {
                        entry.record.status = McpServerStatus::Retrying;
                    }
                    if transport_changed {
                        entry.session = None;
                        entry.retry_attempts = 0;
                        entry.last_checked_instant = None;
                        entry.next_retry_at = if entry.record.enabled {
                            Some(Instant::now())
                        } else {
                            None
                        };
                        entry.urgent_retry = entry.record.enabled;
                        if !entry.record.enabled {
                            entry.record.status = McpServerStatus::Disabled;
                        }
                    }
                }
                None => {
                    state
                        .servers
                        .insert(server_id, ServerRuntimeState::new(record, tools));
                }
            }
        }

        Ok(())
    }

    fn due_server_ids(&self) -> Vec<i64> {
        let now = Instant::now();
        let state = self.state_guard();
        state
            .servers
            .iter()
            .filter_map(|(server_id, entry)| {
                should_poll_server(
                    entry.record.enabled,
                    entry.session.is_some(),
                    entry.record.status,
                    entry.urgent_retry,
                    entry.next_retry_at,
                    entry.last_checked_instant,
                    &self.config,
                    now,
                )
                .then_some(*server_id)
            })
            .collect()
    }

    async fn refresh_server(&self, server_id: i64) -> Result<(), McpRuntimeError> {
        let now = Instant::now();
        let checked_at = Utc::now().to_rfc3339();
        let record = {
            let mut state = self.state_guard();
            let entry = state
                .servers
                .get_mut(&server_id)
                .ok_or(McpRuntimeError::ServerNotFound { server_id })?;
            if !entry.record.enabled {
                entry.record.status = McpServerStatus::Disabled;
                entry.session = None;
                entry.next_retry_at = None;
                entry.urgent_retry = false;
                return Ok(());
            }

            entry.record.status = if entry.retry_attempts > 0 || entry.urgent_retry {
                McpServerStatus::Retrying
            } else {
                McpServerStatus::Connecting
            };
            entry.record.last_checked_at = Some(checked_at.clone());
            entry.last_checked_instant = Some(now);
            entry.urgent_retry = false;
            entry.record.clone()
        };

        match timeout_operation(
            record.timeout_ms,
            self.connector.connect(&record),
            || McpRuntimeError::ConnectFailed {
                server_id,
                reason: format!("connection timed out after {}ms", record.timeout_ms),
            },
            |error| error,
        )
        .await
        {
            Ok(session) => match timeout_operation(
                record.timeout_ms,
                session.list_tools(),
                || McpRuntimeError::ConnectFailed {
                    server_id,
                    reason: format!("tool discovery timed out after {}ms", record.timeout_ms),
                },
                |error| error,
            )
            .await
            {
                Ok(tools) => {
                    let normalized_tools = normalize_tools(server_id, tools);
                    let persisted_record = {
                        let mut state = self.state_guard();
                        let entry = state
                            .servers
                            .get_mut(&server_id)
                            .ok_or(McpRuntimeError::ServerNotFound { server_id })?;
                        entry.session = Some(session);
                        entry.tools = normalized_tools.clone();
                        entry.retry_attempts = 0;
                        entry.next_retry_at = None;
                        entry.record.status = McpServerStatus::Ready;
                        entry.record.last_checked_at = Some(checked_at.clone());
                        entry.record.last_success_at = Some(checked_at.clone());
                        entry.record.last_error = None;
                        entry.record.discovered_tool_count = normalized_tools.len() as u32;
                        entry.record.clone()
                    };

                    self.repo
                        .replace_mcp_server_tools(server_id, &normalized_tools)
                        .await?;
                    self.repo.upsert_mcp_server(&persisted_record).await?;
                }
                Err(error) => {
                    self.handle_connection_failure(server_id, checked_at, error.to_string())
                        .await?;
                }
            },
            Err(error) => {
                self.handle_connection_failure(server_id, checked_at, error.to_string())
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_connection_failure(
        &self,
        server_id: i64,
        checked_at: String,
        reason: String,
    ) -> Result<(), McpRuntimeError> {
        let persisted_record = {
            let mut state = self.state_guard();
            let entry = state
                .servers
                .get_mut(&server_id)
                .ok_or(McpRuntimeError::ServerNotFound { server_id })?;
            entry.session = None;
            entry.retry_attempts = entry.retry_attempts.saturating_add(1);
            let delay = retry_delay(&self.config, entry.retry_attempts);
            entry.next_retry_at = Some(Instant::now() + delay);
            entry.record.status = McpServerStatus::Retrying;
            entry.record.last_checked_at = Some(checked_at);
            entry.record.last_error = Some(reason);
            entry.record.discovered_tool_count = entry.tools.len() as u32;
            entry.record.clone()
        };

        self.repo.upsert_mcp_server(&persisted_record).await?;
        self.notify_supervisor();
        Ok(())
    }

    async fn record_runtime_disconnect(&self, server_id: i64, reason: String) {
        let checked_at = Utc::now().to_rfc3339();
        let persisted_record = {
            let mut state = self.state_guard();
            let Some(entry) = state.servers.get_mut(&server_id) else {
                return;
            };

            if entry.session.is_none() || entry.record.status != McpServerStatus::Ready {
                return;
            }

            entry.session = None;
            entry.retry_attempts = entry.retry_attempts.saturating_add(1);
            let delay = retry_delay(&self.config, entry.retry_attempts);
            entry.next_retry_at = Some(Instant::now() + delay);
            entry.record.status = McpServerStatus::Retrying;
            entry.record.last_checked_at = Some(checked_at);
            entry.record.last_error = Some(reason);
            entry.record.discovered_tool_count = entry.tools.len() as u32;
            entry.record.clone()
        };

        if let Err(error) = self.repo.upsert_mcp_server(&persisted_record).await {
            tracing::warn!(%error, server_id, "failed to persist mcp runtime disconnect");
        }
        self.notify_supervisor();
    }

    fn state_guard(&self) -> MutexGuard<'_, RuntimeState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("mcp runtime mutex was poisoned, recovering state");
                poisoned.into_inner()
            }
        }
    }

    pub(crate) fn next_supervisor_delay(&self) -> Duration {
        let now = Instant::now();
        let state = self.state_guard();
        state.servers.values().fold(
            self.config.supervisor_poll_interval,
            |current_delay, entry| {
                if !entry.record.enabled || entry.record.status == McpServerStatus::Disabled {
                    return current_delay;
                }

                let candidate_delay =
                    if entry.session.is_some() && entry.record.status == McpServerStatus::Ready {
                        entry
                            .last_checked_instant
                            .map(|checked| {
                                self.config
                                    .ready_recheck_interval
                                    .saturating_sub(now.duration_since(checked))
                            })
                            .unwrap_or(Duration::ZERO)
                    } else if let Some(deadline) = entry.next_retry_at {
                        if deadline > now {
                            deadline.duration_since(now)
                        } else {
                            Duration::ZERO
                        }
                    } else if entry.urgent_retry || entry.last_checked_instant.is_none() {
                        Duration::ZERO
                    } else {
                        self.config.supervisor_poll_interval
                    };

                current_delay.min(candidate_delay)
            },
        )
    }

    pub(crate) async fn wait_for_supervisor_wakeup(&self, delay: Duration) {
        let notified = self.supervisor_wakeup.notified();
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = notified => {}
        }
    }

    fn notify_supervisor(&self) {
        self.supervisor_wakeup.notify_one();
    }
}

#[derive(Default)]
pub struct RmcpConnector;

#[async_trait]
impl McpConnector for RmcpConnector {
    async fn connect(
        &self,
        server: &McpServerRecord,
    ) -> Result<Arc<dyn McpSession>, McpRuntimeError> {
        let server_id = server.id.unwrap_or_default();
        let client = match &server.transport {
            McpTransportConfig::Stdio { command, args, env } => {
                let mut process = tokio::process::Command::new(command);
                process.args(args);
                process.envs(env);
                process.kill_on_drop(true);
                let transport = TokioChildProcess::new(process).map_err(|error| {
                    McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: error.to_string(),
                    }
                })?;
                timeout_operation(
                    server.timeout_ms,
                    serve_client((), transport),
                    || McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: format!("connection timed out after {}ms", server.timeout_ms),
                    },
                    |error| McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: error.to_string(),
                    },
                )
                .await?
            }
            McpTransportConfig::Http { url, headers }
            | McpTransportConfig::Sse { url, headers } => {
                let config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                    .custom_headers(parse_headers(headers, server_id)?);
                let transport = StreamableHttpClientTransport::from_config(config);
                timeout_operation(
                    server.timeout_ms,
                    serve_client((), transport),
                    || McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: format!("connection timed out after {}ms", server.timeout_ms),
                    },
                    |error| McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: error.to_string(),
                    },
                )
                .await?
            }
        };

        Ok(Arc::new(RmcpSession::new(server_id, client)))
    }
}

struct RmcpSession {
    server_id: i64,
    client: tokio::sync::Mutex<RunningService<RoleClient, ()>>,
}

impl RmcpSession {
    fn new(server_id: i64, client: RunningService<RoleClient, ()>) -> Self {
        Self {
            server_id,
            client: tokio::sync::Mutex::new(client),
        }
    }
}

#[async_trait]
impl McpSession for RmcpSession {
    async fn list_tools(&self) -> Result<Vec<McpDiscoveredToolRecord>, McpRuntimeError> {
        let peer = {
            let client = self.client.lock().await;
            client.peer().clone()
        };
        let tools =
            peer.list_all_tools()
                .await
                .map_err(|error| McpRuntimeError::ConnectFailed {
                    server_id: self.server_id,
                    reason: error.to_string(),
                })?;

        tools
            .into_iter()
            .map(|tool| rmcp_tool_to_record(self.server_id, tool))
            .collect()
    }

    async fn call_tool(
        &self,
        tool_name_original: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let arguments = match input {
            serde_json::Value::Object(arguments) => arguments,
            serde_json::Value::Null => serde_json::Map::new(),
            other => {
                return Err(McpRuntimeError::ToolCallFailed {
                    server_id: self.server_id,
                    tool_name: tool_name_original.to_string(),
                    reason: format!("tool arguments must be a JSON object, got {other}"),
                });
            }
        };

        let peer = {
            let client = self.client.lock().await;
            client.peer().clone()
        };
        let params =
            CallToolRequestParams::new(tool_name_original.to_string()).with_arguments(arguments);
        let result =
            peer.call_tool(params)
                .await
                .map_err(|error| McpRuntimeError::ConnectFailed {
                    server_id: self.server_id,
                    reason: error.to_string(),
                })?;

        call_tool_result_to_json(self.server_id, tool_name_original, result)
    }
}

fn normalize_tools(
    server_id: i64,
    tools: Vec<McpDiscoveredToolRecord>,
) -> Vec<McpDiscoveredToolRecord> {
    tools
        .into_iter()
        .map(|mut tool| {
            tool.server_id = server_id;
            tool
        })
        .collect()
}

fn parse_headers(
    headers: &std::collections::BTreeMap<String, String>,
    server_id: i64,
) -> Result<HashMap<HeaderName, HeaderValue>, McpRuntimeError> {
    let mut parsed = HashMap::with_capacity(headers.len());
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            McpRuntimeError::ConnectFailed {
                server_id,
                reason: format!("invalid header name '{name}': {error}"),
            }
        })?;
        let header_value =
            HeaderValue::from_str(value).map_err(|error| McpRuntimeError::ConnectFailed {
                server_id,
                reason: format!("invalid header value for '{name}': {error}"),
            })?;
        parsed.insert(header_name, header_value);
    }
    Ok(parsed)
}

fn rmcp_tool_to_record(
    server_id: i64,
    tool: RmcpTool,
) -> Result<McpDiscoveredToolRecord, McpRuntimeError> {
    let description = tool
        .description
        .map(std::borrow::Cow::into_owned)
        .or(tool.title)
        .unwrap_or_else(|| tool.name.to_string());
    let annotations = tool
        .annotations
        .map(|annotations| {
            serde_json::to_value(annotations).map_err(|error| McpRuntimeError::Serialization {
                reason: error.to_string(),
            })
        })
        .transpose()?;

    Ok(McpDiscoveredToolRecord {
        server_id,
        tool_name_original: tool.name.into_owned(),
        description,
        schema: serde_json::Value::Object((*tool.input_schema).clone()),
        annotations,
    })
}

fn call_tool_result_to_json(
    server_id: i64,
    tool_name: &str,
    result: rmcp::model::CallToolResult,
) -> Result<serde_json::Value, McpRuntimeError> {
    if result.is_error.unwrap_or(false) {
        return Err(McpRuntimeError::ToolCallFailed {
            server_id,
            tool_name: tool_name.to_string(),
            reason: summarize_call_tool_result(&result),
        });
    }

    match (result.structured_content, result.content.is_empty()) {
        (Some(structured_content), true) => Ok(structured_content),
        (Some(structured_content), false) => Ok(serde_json::json!({
            "structured_content": structured_content,
            "content": result.content,
        })),
        (None, _) => Ok(serde_json::json!({
            "content": result.content,
        })),
    }
}

fn summarize_call_tool_result(result: &rmcp::model::CallToolResult) -> String {
    if let Some(structured_content) = &result.structured_content {
        return structured_content.to_string();
    }

    if !result.content.is_empty() {
        return serde_json::to_string(&result.content)
            .unwrap_or_else(|error| format!("failed to serialize tool error content: {error}"));
    }

    "tool call returned an error without details".to_string()
}

async fn timeout_operation<T, E, F>(
    timeout_ms: u64,
    future: F,
    on_timeout: impl FnOnce() -> McpRuntimeError,
    map_error: impl FnOnce(E) -> McpRuntimeError,
) -> Result<T, McpRuntimeError>
where
    F: Future<Output = Result<T, E>>,
{
    match tokio::time::timeout(Duration::from_millis(timeout_ms.max(1)), future).await {
        Ok(output) => output.map_err(map_error),
        Err(_) => Err(on_timeout()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use tokio::sync::Mutex;

    use argus_protocol::{AgentMcpServerBinding, McpTransportConfig};

    use super::*;

    fn server(id: i64, display_name: &str, enabled: bool) -> McpServerRecord {
        McpServerRecord {
            id: Some(id),
            display_name: display_name.to_string(),
            enabled,
            transport: McpTransportConfig::Stdio {
                command: "mcp-server".to_string(),
                args: vec!["--stdio".to_string()],
                env: Default::default(),
            },
            timeout_ms: 30_000,
            status: if enabled {
                McpServerStatus::Failed
            } else {
                McpServerStatus::Disabled
            },
            last_checked_at: None,
            last_success_at: None,
            last_error: None,
            discovered_tool_count: 0,
        }
    }

    fn tool(server_id: i64, name: &str, description: &str) -> McpDiscoveredToolRecord {
        McpDiscoveredToolRecord {
            server_id,
            tool_name_original: name.to_string(),
            description: description.to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            }),
            annotations: Some(serde_json::json!({ "title": name })),
        }
    }

    #[derive(Default)]
    struct FakeRepo {
        servers: Mutex<HashMap<i64, McpServerRecord>>,
        server_tools: Mutex<HashMap<i64, Vec<McpDiscoveredToolRecord>>>,
        agent_bindings: Mutex<HashMap<AgentId, Vec<argus_protocol::AgentMcpBinding>>>,
        replace_calls: Mutex<Vec<(i64, Vec<String>)>>,
    }

    impl FakeRepo {
        fn new(servers: Vec<McpServerRecord>) -> Self {
            Self {
                servers: Mutex::new(
                    servers
                        .into_iter()
                        .map(|server| (server.id.expect("test server should have an id"), server))
                        .collect(),
                ),
                ..Default::default()
            }
        }

        async fn set_agent_bindings(
            &self,
            agent_id: AgentId,
            bindings: Vec<argus_protocol::AgentMcpBinding>,
        ) {
            self.agent_bindings.lock().await.insert(agent_id, bindings);
        }

        async fn set_existing_tools(&self, server_id: i64, tools: Vec<McpDiscoveredToolRecord>) {
            self.server_tools.lock().await.insert(server_id, tools);
        }

        async fn tools_for_server(&self, server_id: i64) -> Vec<McpDiscoveredToolRecord> {
            self.server_tools
                .lock()
                .await
                .get(&server_id)
                .cloned()
                .unwrap_or_default()
        }

        async fn replace_log(&self) -> Vec<(i64, Vec<String>)> {
            self.replace_calls.lock().await.clone()
        }
    }

    #[async_trait]
    impl McpRepository for FakeRepo {
        async fn upsert_mcp_server(
            &self,
            record: &McpServerRecord,
        ) -> Result<i64, argus_repository::DbError> {
            let server_id = record.id.expect("test server should have an id");
            self.servers.lock().await.insert(server_id, record.clone());
            Ok(server_id)
        }

        async fn get_mcp_server(
            &self,
            id: i64,
        ) -> Result<Option<McpServerRecord>, argus_repository::DbError> {
            Ok(self.servers.lock().await.get(&id).cloned())
        }

        async fn list_mcp_servers(
            &self,
        ) -> Result<Vec<McpServerRecord>, argus_repository::DbError> {
            let mut servers = self
                .servers
                .lock()
                .await
                .values()
                .cloned()
                .collect::<Vec<_>>();
            servers.sort_by_key(|server| server.id);
            Ok(servers)
        }

        async fn delete_mcp_server(&self, id: i64) -> Result<bool, argus_repository::DbError> {
            Ok(self.servers.lock().await.remove(&id).is_some())
        }

        async fn replace_mcp_server_tools(
            &self,
            server_id: i64,
            tools: &[McpDiscoveredToolRecord],
        ) -> Result<(), argus_repository::DbError> {
            self.server_tools
                .lock()
                .await
                .insert(server_id, tools.to_vec());
            self.replace_calls.lock().await.push((
                server_id,
                tools
                    .iter()
                    .map(|tool| tool.tool_name_original.clone())
                    .collect(),
            ));
            Ok(())
        }

        async fn list_mcp_server_tools(
            &self,
            server_id: i64,
        ) -> Result<Vec<McpDiscoveredToolRecord>, argus_repository::DbError> {
            Ok(self
                .server_tools
                .lock()
                .await
                .get(&server_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn set_agent_mcp_bindings(
            &self,
            agent_id: AgentId,
            bindings: &[argus_protocol::AgentMcpBinding],
        ) -> Result<(), argus_repository::DbError> {
            self.agent_bindings
                .lock()
                .await
                .insert(agent_id, bindings.to_vec());
            Ok(())
        }

        async fn list_agent_mcp_bindings(
            &self,
            agent_id: AgentId,
        ) -> Result<Vec<argus_protocol::AgentMcpBinding>, argus_repository::DbError> {
            Ok(self
                .agent_bindings
                .lock()
                .await
                .get(&agent_id)
                .cloned()
                .unwrap_or_default())
        }
    }

    enum ConnectOutcome {
        Success(FakeSession),
        Fail(String),
        Sleep(Duration),
    }

    struct FakeSession {
        tools: Vec<McpDiscoveredToolRecord>,
        list_tools_delay: Option<Duration>,
        list_tools_error: Option<String>,
        call_tool_delay: Option<Duration>,
        call_tool_error: Option<McpRuntimeError>,
    }

    impl FakeSession {
        fn success(tools: Vec<McpDiscoveredToolRecord>) -> Self {
            Self {
                tools,
                list_tools_delay: None,
                list_tools_error: None,
                call_tool_delay: None,
                call_tool_error: None,
            }
        }
    }

    #[async_trait]
    impl McpSession for FakeSession {
        async fn list_tools(&self) -> Result<Vec<McpDiscoveredToolRecord>, McpRuntimeError> {
            if let Some(delay) = self.list_tools_delay {
                tokio::time::sleep(delay).await;
            }
            if let Some(reason) = &self.list_tools_error {
                return Err(McpRuntimeError::ConnectFailed {
                    server_id: self
                        .tools
                        .first()
                        .map(|tool| tool.server_id)
                        .unwrap_or_default(),
                    reason: reason.clone(),
                });
            }
            Ok(self.tools.clone())
        }

        async fn call_tool(
            &self,
            _tool_name_original: &str,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, McpRuntimeError> {
            if let Some(delay) = self.call_tool_delay {
                tokio::time::sleep(delay).await;
            }
            if let Some(error) = &self.call_tool_error {
                return Err(match error {
                    McpRuntimeError::ConnectFailed { server_id, reason } => {
                        McpRuntimeError::ConnectFailed {
                            server_id: *server_id,
                            reason: reason.clone(),
                        }
                    }
                    McpRuntimeError::ToolCallFailed {
                        server_id,
                        tool_name,
                        reason,
                    } => McpRuntimeError::ToolCallFailed {
                        server_id: *server_id,
                        tool_name: tool_name.clone(),
                        reason: reason.clone(),
                    },
                    McpRuntimeError::ServerNotFound { server_id } => {
                        McpRuntimeError::ServerNotFound {
                            server_id: *server_id,
                        }
                    }
                    McpRuntimeError::ServerNotReady { server_id } => {
                        McpRuntimeError::ServerNotReady {
                            server_id: *server_id,
                        }
                    }
                    McpRuntimeError::Repository { reason } => McpRuntimeError::Repository {
                        reason: reason.clone(),
                    },
                    McpRuntimeError::InvalidConfiguration {
                        display_name,
                        reason,
                    } => McpRuntimeError::InvalidConfiguration {
                        display_name: display_name.clone(),
                        reason: reason.clone(),
                    },
                    McpRuntimeError::Serialization { reason } => McpRuntimeError::Serialization {
                        reason: reason.clone(),
                    },
                });
            }
            Ok(input)
        }
    }

    struct FakeConnector {
        plans: Mutex<HashMap<i64, VecDeque<ConnectOutcome>>>,
        attempts: Mutex<HashMap<i64, usize>>,
    }

    impl FakeConnector {
        fn new() -> Self {
            Self {
                plans: Mutex::new(HashMap::new()),
                attempts: Mutex::new(HashMap::new()),
            }
        }

        async fn push_success(&self, server_id: i64, tools: Vec<McpDiscoveredToolRecord>) {
            self.plans
                .lock()
                .await
                .entry(server_id)
                .or_default()
                .push_back(ConnectOutcome::Success(FakeSession::success(tools)));
        }

        async fn push_session(&self, server_id: i64, session: FakeSession) {
            self.plans
                .lock()
                .await
                .entry(server_id)
                .or_default()
                .push_back(ConnectOutcome::Success(session));
        }

        async fn push_fail(&self, server_id: i64, reason: &str) {
            self.plans
                .lock()
                .await
                .entry(server_id)
                .or_default()
                .push_back(ConnectOutcome::Fail(reason.to_string()));
        }

        async fn push_sleep(&self, server_id: i64, duration: Duration) {
            self.plans
                .lock()
                .await
                .entry(server_id)
                .or_default()
                .push_back(ConnectOutcome::Sleep(duration));
        }

        async fn attempts(&self, server_id: i64) -> usize {
            *self.attempts.lock().await.get(&server_id).unwrap_or(&0)
        }
    }

    #[async_trait]
    impl McpConnector for FakeConnector {
        async fn connect(
            &self,
            server: &McpServerRecord,
        ) -> Result<Arc<dyn McpSession>, McpRuntimeError> {
            let server_id = server.id.unwrap_or_default();
            *self.attempts.lock().await.entry(server_id).or_insert(0) += 1;

            match self
                .plans
                .lock()
                .await
                .entry(server_id)
                .or_default()
                .pop_front()
            {
                Some(ConnectOutcome::Success(session)) => Ok(Arc::new(session)),
                Some(ConnectOutcome::Fail(reason)) => {
                    Err(McpRuntimeError::ConnectFailed { server_id, reason })
                }
                Some(ConnectOutcome::Sleep(duration)) => {
                    tokio::time::sleep(duration).await;
                    Err(McpRuntimeError::ConnectFailed {
                        server_id,
                        reason: "delayed connect never completed".to_string(),
                    })
                }
                None => Err(McpRuntimeError::ConnectFailed {
                    server_id,
                    reason: "no connection plan configured".to_string(),
                }),
            }
        }
    }

    #[tokio::test]
    async fn initial_load_of_enabled_servers() {
        let repo = Arc::new(FakeRepo::new(vec![server(1, "Slack", true)]));
        let connector = Arc::new(FakeConnector::new());
        connector
            .push_success(1, vec![tool(1, "post_message", "Send a message")])
            .await;

        let runtime = Arc::new(McpRuntime::new(
            repo.clone(),
            connector.clone(),
            McpRuntimeConfig::default(),
        ));
        runtime.poll_once().await.expect("poll should succeed");

        let snapshot = runtime
            .server_snapshot(1)
            .expect("server snapshot should exist");
        assert_eq!(snapshot.status, McpServerStatus::Ready);
        assert_eq!(snapshot.retry_attempts, 0);
        assert_eq!(connector.attempts(1).await, 1);
        assert_eq!(repo.tools_for_server(1).await.len(), 1);
        assert_eq!(repo.replace_log().await.len(), 1);
    }

    #[tokio::test]
    async fn failed_server_enters_retrying_with_backoff() {
        let repo = Arc::new(FakeRepo::new(vec![server(2, "GitHub", true)]));
        let connector = Arc::new(FakeConnector::new());
        connector.push_fail(2, "socket closed").await;

        let config = McpRuntimeConfig {
            initial_retry_delay: Duration::from_secs(1),
            ..McpRuntimeConfig::default()
        };
        let runtime = Arc::new(McpRuntime::new(repo, connector.clone(), config));

        runtime.poll_once().await.expect("poll should succeed");
        let snapshot = runtime
            .server_snapshot(2)
            .expect("server snapshot should exist");

        assert_eq!(snapshot.status, McpServerStatus::Retrying);
        assert_eq!(snapshot.retry_attempts, 1);
        let next_retry_delay = snapshot
            .next_retry_delay
            .expect("retrying server should report a next retry delay");
        assert!(next_retry_delay <= Duration::from_secs(1));
        assert!(next_retry_delay > Duration::from_millis(900));
        assert_eq!(connector.attempts(2).await, 1);
    }

    #[tokio::test]
    async fn successful_reconnect_refreshes_discovery_snapshot() {
        let repo = Arc::new(FakeRepo::new(vec![server(3, "Docs", true)]));
        repo.set_existing_tools(3, vec![tool(3, "old_tool", "Old tool")])
            .await;

        let connector = Arc::new(FakeConnector::new());
        connector.push_fail(3, "temporary failure").await;
        connector
            .push_success(3, vec![tool(3, "new_tool", "New tool")])
            .await;

        let config = McpRuntimeConfig {
            initial_retry_delay: Duration::ZERO,
            ..McpRuntimeConfig::default()
        };
        let runtime = Arc::new(McpRuntime::new(repo.clone(), connector, config));

        runtime
            .poll_once()
            .await
            .expect("first poll should succeed");
        runtime
            .poll_once()
            .await
            .expect("second poll should succeed");

        let stored = repo.tools_for_server(3).await;
        assert_eq!(stored, vec![tool(3, "new_tool", "New tool")]);
        let snapshot = runtime
            .server_snapshot(3)
            .expect("server snapshot should exist");
        assert_eq!(snapshot.status, McpServerStatus::Ready);
        assert_eq!(snapshot.retry_attempts, 0);
    }

    #[tokio::test]
    async fn resolve_for_agent_returns_only_ready_tools() {
        let repo = Arc::new(FakeRepo::new(vec![
            server(4, "Slack", true),
            server(5, "GitHub", true),
        ]));
        repo.set_agent_bindings(
            AgentId::new(7),
            vec![
                argus_protocol::AgentMcpBinding {
                    server: AgentMcpServerBinding {
                        agent_id: AgentId::new(7),
                        server_id: 4,
                    },
                    allowed_tools: None,
                },
                argus_protocol::AgentMcpBinding {
                    server: AgentMcpServerBinding {
                        agent_id: AgentId::new(7),
                        server_id: 5,
                    },
                    allowed_tools: None,
                },
            ],
        )
        .await;

        let connector = Arc::new(FakeConnector::new());
        connector
            .push_success(4, vec![tool(4, "post_message", "Send a message")])
            .await;
        connector.push_fail(5, "offline").await;

        let runtime = Arc::new(McpRuntime::new(
            repo,
            connector,
            McpRuntimeConfig::default(),
        ));
        let handle = McpRuntime::handle(&runtime);
        runtime.poll_once().await.expect("poll should succeed");

        let resolved = handle
            .resolve_for_agent(AgentId::new(7))
            .await
            .expect("resolve should succeed");
        assert_eq!(resolved.tools.len(), 1);
        assert_eq!(resolved.unavailable_servers.len(), 1);
    }

    #[tokio::test]
    async fn unresolved_server_requests_retry_but_does_not_foreground_connect() {
        let repo = Arc::new(FakeRepo::new(vec![server(6, "Slack", true)]));
        repo.set_agent_bindings(
            AgentId::new(9),
            vec![argus_protocol::AgentMcpBinding {
                server: AgentMcpServerBinding {
                    agent_id: AgentId::new(9),
                    server_id: 6,
                },
                allowed_tools: None,
            }],
        )
        .await;

        let connector = Arc::new(FakeConnector::new());
        let config = McpRuntimeConfig {
            initial_retry_delay: Duration::from_secs(10),
            ..McpRuntimeConfig::default()
        };
        let runtime = Arc::new(McpRuntime::new(repo, connector.clone(), config));
        let handle = McpRuntime::handle(&runtime);

        let resolved = handle
            .resolve_for_agent(AgentId::new(9))
            .await
            .expect("resolve should succeed");
        assert!(resolved.tools.is_empty());
        assert_eq!(connector.attempts(6).await, 0);

        runtime
            .poll_once()
            .await
            .expect("retry scheduling should not force an immediate reconnect");
        assert_eq!(connector.attempts(6).await, 0);

        let snapshot = runtime
            .server_snapshot(6)
            .expect("server snapshot should exist");
        assert_eq!(snapshot.status, McpServerStatus::Retrying);
        let next_retry_delay = snapshot
            .next_retry_delay
            .expect("retrying server should report a next retry delay");
        assert!(next_retry_delay <= Duration::from_secs(10));
        assert!(next_retry_delay > Duration::from_secs(9));
    }

    #[tokio::test]
    async fn test_server_input_returns_failed_result_when_list_tools_fails() {
        let repo = Arc::new(FakeRepo::default());
        let connector = Arc::new(FakeConnector::new());
        connector
            .push_session(
                0,
                FakeSession {
                    tools: Vec::new(),
                    list_tools_delay: None,
                    list_tools_error: Some("tool discovery failed".to_string()),
                    call_tool_delay: None,
                    call_tool_error: None,
                },
            )
            .await;

        let runtime = McpRuntime::new(repo, connector, McpRuntimeConfig::default());
        let result = runtime
            .test_server_input(McpServerRecord {
                id: None,
                ..server(0, "Unsaved", true)
            })
            .await
            .expect("test connection should return a structured result");

        assert_eq!(result.status, McpServerStatus::Failed);
        assert!(result.message.contains("tool discovery failed"));
    }

    #[tokio::test]
    async fn poll_once_marks_server_retrying_when_connect_times_out() {
        let repo = Arc::new(FakeRepo::new(vec![McpServerRecord {
            timeout_ms: 10,
            ..server(8, "Slow", true)
        }]));
        let connector = Arc::new(FakeConnector::new());
        connector.push_sleep(8, Duration::from_millis(50)).await;

        let runtime = Arc::new(McpRuntime::new(
            repo,
            connector.clone(),
            McpRuntimeConfig::default(),
        ));
        let started = Instant::now();

        runtime
            .poll_once()
            .await
            .expect("poll should degrade, not fail");

        let snapshot = runtime
            .server_snapshot(8)
            .expect("server snapshot should exist");
        assert_eq!(snapshot.status, McpServerStatus::Retrying);
        assert_eq!(connector.attempts(8).await, 1);
        assert!(started.elapsed() < Duration::from_millis(40));
    }

    #[tokio::test]
    async fn transport_failures_during_tool_execution_mark_server_for_retry() {
        let repo = Arc::new(FakeRepo::new(vec![server(10, "Slack", true)]));
        let connector = Arc::new(FakeConnector::new());
        connector
            .push_session(
                10,
                FakeSession {
                    tools: vec![tool(10, "post_message", "Send a message")],
                    list_tools_delay: None,
                    list_tools_error: None,
                    call_tool_delay: None,
                    call_tool_error: Some(McpRuntimeError::ConnectFailed {
                        server_id: 10,
                        reason: "broken pipe".to_string(),
                    }),
                },
            )
            .await;

        let runtime = Arc::new(McpRuntime::new(
            repo,
            connector,
            McpRuntimeConfig::default(),
        ));
        runtime.poll_once().await.expect("poll should succeed");

        let error = runtime
            .call_tool(10, "post_message", serde_json::json!({ "text": "hello" }))
            .await
            .expect_err("broken sessions should fail tool execution");
        assert!(matches!(error, McpRuntimeError::ConnectFailed { .. }));

        let snapshot = runtime
            .server_snapshot(10)
            .expect("server snapshot should exist");
        assert_eq!(snapshot.status, McpServerStatus::Retrying);
    }
}
