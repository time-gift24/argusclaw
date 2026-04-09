use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Response, Url};
use rmcp::model::{CallToolRequestParams, ClientInfo, ProtocolVersion, Tool as RmcpTool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
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

const LEGACY_SSE_PROTOCOL_VERSION: &str = "2024-11-05";
const LEGACY_SSE_CLIENT_NAME: &str = "argusclaw";
const STREAMABLE_HTTP_ACCEPT: &str = "application/json, text/event-stream";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

type PendingLegacySseResponse = Result<serde_json::Value, McpRuntimeError>;
type PendingLegacySseSender = tokio::sync::oneshot::Sender<PendingLegacySseResponse>;
type PendingLegacySseMap = HashMap<u64, PendingLegacySseSender>;

fn streamable_http_protocol_versions() -> [ProtocolVersion; 3] {
    [
        ProtocolVersion::V_2025_06_18,
        ProtocolVersion::V_2025_03_26,
        ProtocolVersion::V_2024_11_05,
    ]
}

fn build_client_info(protocol_version: ProtocolVersion) -> ClientInfo {
    ClientInfo::default().with_protocol_version(protocol_version)
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

    #[must_use]
    pub fn supervisor_started(&self) -> bool {
        self.supervisor_started.load(Ordering::SeqCst)
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
                    serve_client(build_client_info(ProtocolVersion::V_2025_06_18), transport),
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
            McpTransportConfig::Http { url, headers } => {
                return Ok(Arc::new(
                    StreamableHttpSession::connect(server_id, url, headers).await?,
                ));
            }
            McpTransportConfig::Sse { url, headers } => {
                return Ok(Arc::new(
                    LegacySseSession::connect(server_id, url, headers).await?,
                ));
            }
        };

        Ok(Arc::new(RmcpSession::new(server_id, client)))
    }
}

struct LegacySseSession {
    server_id: i64,
    state: Arc<LegacySseSessionState>,
    stream_task: tokio::task::JoinHandle<()>,
}

struct LegacySseSessionState {
    client: Client,
    message_url: tokio::sync::Mutex<Option<Url>>,
    pending: tokio::sync::Mutex<PendingLegacySseMap>,
    closed_reason: tokio::sync::Mutex<Option<String>>,
    state_changed: tokio::sync::Notify,
    next_request_id: AtomicU64,
}

impl LegacySseSessionState {
    fn new(client: Client) -> Self {
        Self {
            client,
            message_url: tokio::sync::Mutex::new(None),
            pending: tokio::sync::Mutex::new(HashMap::new()),
            closed_reason: tokio::sync::Mutex::new(None),
            state_changed: tokio::sync::Notify::new(),
            next_request_id: AtomicU64::new(1),
        }
    }

    async fn wait_for_message_url(&self, server_id: i64) -> Result<Url, McpRuntimeError> {
        loop {
            let notified = self.state_changed.notified();
            if let Some(url) = self.message_url.lock().await.clone() {
                return Ok(url);
            }
            if let Some(reason) = self.closed_reason.lock().await.clone() {
                return Err(McpRuntimeError::ConnectFailed { server_id, reason });
            }
            notified.await;
        }
    }

    async fn set_message_url(&self, message_url: Url) {
        let mut stored_url = self.message_url.lock().await;
        if stored_url.is_none() {
            *stored_url = Some(message_url);
            self.state_changed.notify_waiters();
        }
    }

    async fn closed_reason(&self) -> Option<String> {
        self.closed_reason.lock().await.clone()
    }

    async fn fail_pending(&self, error: McpRuntimeError) {
        let pending = {
            let mut pending = self.pending.lock().await;
            std::mem::take(&mut *pending)
        };

        for sender in pending.into_values() {
            let send_error = match &error {
                McpRuntimeError::Repository { reason } => McpRuntimeError::Repository {
                    reason: reason.clone(),
                },
                McpRuntimeError::ServerNotFound { server_id } => McpRuntimeError::ServerNotFound {
                    server_id: *server_id,
                },
                McpRuntimeError::ServerNotReady { server_id } => McpRuntimeError::ServerNotReady {
                    server_id: *server_id,
                },
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
            };
            let _ = sender.send(Err(send_error));
        }
    }

    async fn mark_closed(&self, server_id: i64, reason: String) {
        let mut closed_reason = self.closed_reason.lock().await;
        if closed_reason.is_some() {
            return;
        }

        *closed_reason = Some(reason.clone());
        drop(closed_reason);
        self.fail_pending(McpRuntimeError::ConnectFailed { server_id, reason })
            .await;
        self.state_changed.notify_waiters();
    }

    async fn register_pending(
        &self,
        server_id: i64,
        request_id: u64,
    ) -> Result<tokio::sync::oneshot::Receiver<PendingLegacySseResponse>, McpRuntimeError> {
        if let Some(reason) = self.closed_reason().await {
            return Err(McpRuntimeError::ConnectFailed { server_id, reason });
        }

        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(request_id, sender);
        Ok(receiver)
    }

    async fn remove_pending(&self, request_id: u64) {
        self.pending.lock().await.remove(&request_id);
    }

    async fn deliver_response(&self, message: serde_json::Value) {
        let Some(request_id) = message.get("id").and_then(serde_json::Value::as_u64) else {
            return;
        };

        let sender = self.pending.lock().await.remove(&request_id);
        if let Some(sender) = sender {
            let _ = sender.send(Ok(message));
        }
    }
}

impl LegacySseSession {
    async fn connect(
        server_id: i64,
        sse_url: &str,
        headers: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, McpRuntimeError> {
        let client = build_reqwest_client(headers, server_id)?;
        let sse_url = Url::parse(sse_url).map_err(|error| McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!("invalid SSE URL '{sse_url}': {error}"),
        })?;
        let response = client
            .get(sse_url.clone())
            .header(ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|error| McpRuntimeError::ConnectFailed {
                server_id,
                reason: error.to_string(),
            })?;

        ensure_legacy_sse_response(&response, server_id)?;

        let state = Arc::new(LegacySseSessionState::new(client));
        let stream_task = tokio::spawn(run_legacy_sse_stream(
            server_id,
            sse_url,
            response,
            Arc::clone(&state),
        ));
        let session = Self {
            server_id,
            state,
            stream_task,
        };

        session.state.wait_for_message_url(server_id).await?;
        session
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": LEGACY_SSE_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": LEGACY_SSE_CLIENT_NAME,
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
            )
            .await?;
        session
            .notify("notifications/initialized", serde_json::json!({}))
            .await?;

        Ok(session)
    }

    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let request_id = self.state.next_request_id.fetch_add(1, Ordering::SeqCst);
        let response_receiver = self
            .state
            .register_pending(self.server_id, request_id)
            .await
            .map_err(|error| match error {
                McpRuntimeError::ConnectFailed { reason, .. } => McpRuntimeError::ConnectFailed {
                    server_id: self.server_id,
                    reason,
                },
                other => other,
            })?;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });

        if let Err(error) = self.post_message(request).await {
            self.state.remove_pending(request_id).await;
            return Err(error);
        }

        match response_receiver.await {
            Ok(Ok(response)) => extract_json_rpc_result(self.server_id, response),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: format!("legacy SSE response channel closed while waiting for '{method}'"),
            }),
        }
    }

    async fn notify(&self, method: &str, params: serde_json::Value) -> Result<(), McpRuntimeError> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.post_message(notification).await
    }

    async fn post_message(&self, message: serde_json::Value) -> Result<(), McpRuntimeError> {
        if let Some(reason) = self.state.closed_reason().await {
            return Err(McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason,
            });
        }

        let message_url = self.state.wait_for_message_url(self.server_id).await?;
        let response = self
            .state
            .client
            .post(message_url)
            .header(CONTENT_TYPE, "application/json")
            .json(&message)
            .send()
            .await
            .map_err(|error| McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: error.to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: format!("unexpected server response: HTTP {status}: {body}"),
            });
        }

        handle_inline_legacy_sse_response(&self.state, response, self.server_id).await
    }
}

impl Drop for LegacySseSession {
    fn drop(&mut self) {
        self.stream_task.abort();
    }
}

struct StreamableHttpSession {
    server_id: i64,
    client: Client,
    url: Url,
    session_id: tokio::sync::Mutex<Option<String>>,
    protocol_version: tokio::sync::Mutex<Option<String>>,
    request_lock: tokio::sync::Mutex<()>,
    next_request_id: AtomicU64,
}

impl StreamableHttpSession {
    async fn connect(
        server_id: i64,
        url: &str,
        headers: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, McpRuntimeError> {
        let client = build_reqwest_client(headers, server_id)?;
        let url = Url::parse(url).map_err(|error| McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!("invalid streamable HTTP URL '{url}': {error}"),
        })?;
        let session = Self {
            server_id,
            client,
            url,
            session_id: tokio::sync::Mutex::new(None),
            protocol_version: tokio::sync::Mutex::new(None),
            request_lock: tokio::sync::Mutex::new(()),
            next_request_id: AtomicU64::new(1),
        };

        let mut failures = Vec::new();
        for protocol_version in streamable_http_protocol_versions() {
            match session
                .initialize_with_protocol_version(protocol_version.as_str())
                .await
            {
                Ok(()) => return Ok(session),
                Err(error) => failures.push(format!("{protocol_version}: {error}")),
            }
        }

        Err(McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!(
                "streamable HTTP handshake failed across protocol versions: {}",
                failures.join(" | ")
            ),
        })
    }

    async fn initialize_with_protocol_version(
        &self,
        protocol_version: &str,
    ) -> Result<(), McpRuntimeError> {
        {
            *self.session_id.lock().await = None;
            *self.protocol_version.lock().await = None;
        }

        let initialize_result = self
            .send_request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": protocol_version,
                    "capabilities": {},
                    "clientInfo": {
                        "name": LEGACY_SSE_CLIENT_NAME,
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
                None,
                false,
            )
            .await?;

        let negotiated_protocol = initialize_result
            .get("protocolVersion")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(protocol_version)
            .to_string();
        *self.protocol_version.lock().await = Some(negotiated_protocol);

        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await
    }

    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
        protocol_version_override: Option<&str>,
        skip_session: bool,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });

        let response = self
            .post_message(request, protocol_version_override, skip_session)
            .await?;
        extract_json_rpc_result(self.server_id, response)
    }

    async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpRuntimeError> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.post_message(notification, None, false).await?;
        Ok(())
    }

    async fn post_message(
        &self,
        message: serde_json::Value,
        protocol_version_override: Option<&str>,
        skip_session: bool,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let mut request = self
            .client
            .post(self.url.clone())
            .header(ACCEPT, STREAMABLE_HTTP_ACCEPT)
            .header(CONTENT_TYPE, "application/json")
            .json(&message);

        if let Some(protocol_version) = protocol_version_override {
            request = request.header(MCP_PROTOCOL_VERSION_HEADER, protocol_version);
        } else if let Some(protocol_version) = self.protocol_version.lock().await.clone() {
            request = request.header(MCP_PROTOCOL_VERSION_HEADER, protocol_version);
        }

        if !skip_session && let Some(session_id) = self.session_id.lock().await.clone() {
            request = request.header(MCP_SESSION_ID_HEADER, session_id);
        }

        let response = request
            .send()
            .await
            .map_err(|error| McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: error.to_string(),
            })?;
        self.handle_http_response(response).await
    }

    async fn handle_http_response(
        &self,
        response: Response,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let status = response.status();
        let session_id = response
            .headers()
            .get(MCP_SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let bytes = response
            .bytes()
            .await
            .map_err(|error| McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: error.to_string(),
            })?;

        if let Some(session_id) = session_id {
            *self.session_id.lock().await = Some(session_id);
        }

        if !status.is_success() {
            return Err(McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: format_http_error(status, &content_type, &bytes),
            });
        }

        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }

        parse_streamable_http_payload(self.server_id, &content_type, &bytes)
    }
}

#[async_trait]
impl McpSession for StreamableHttpSession {
    async fn list_tools(&self) -> Result<Vec<McpDiscoveredToolRecord>, McpRuntimeError> {
        let _request_guard = self.request_lock.lock().await;
        let result = self
            .send_request("tools/list", serde_json::json!({}), None, false)
            .await?;
        let tools = result
            .get("tools")
            .cloned()
            .ok_or(McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: "streamable HTTP tools/list response missing tools".to_string(),
            })?;
        let parsed_tools = serde_json::from_value::<Vec<RmcpTool>>(tools).map_err(|error| {
            McpRuntimeError::Serialization {
                reason: error.to_string(),
            }
        })?;

        parsed_tools
            .into_iter()
            .map(|tool| rmcp_tool_to_record(self.server_id, tool))
            .collect()
    }

    async fn call_tool(
        &self,
        tool_name_original: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError> {
        let _request_guard = self.request_lock.lock().await;
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
        let result = self
            .send_request(
                "tools/call",
                serde_json::json!({
                    "name": tool_name_original,
                    "arguments": arguments,
                }),
                None,
                false,
            )
            .await?;
        let call_tool_result = serde_json::from_value::<rmcp::model::CallToolResult>(result)
            .map_err(|error| McpRuntimeError::Serialization {
                reason: error.to_string(),
            })?;

        call_tool_result_to_json(self.server_id, tool_name_original, call_tool_result)
    }
}

async fn run_legacy_sse_stream(
    server_id: i64,
    sse_url: Url,
    response: Response,
    state: Arc<LegacySseSessionState>,
) {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let stream_result = 'stream: loop {
        match stream.next().await {
            Some(Ok(chunk)) => {
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(event) = take_sse_event(&mut buffer) {
                    if let Err(reason) =
                        handle_legacy_sse_event(server_id, &sse_url, &state, &event).await
                    {
                        break 'stream Err(reason);
                    }
                }
            }
            Some(Err(error)) => break 'stream Err(error.to_string()),
            None => break 'stream Err("legacy SSE stream closed".to_string()),
        }
    };

    let reason = match stream_result {
        Ok(()) => "legacy SSE stream closed".to_string(),
        Err(reason) => reason,
    };
    state.mark_closed(server_id, reason).await;
}

async fn handle_legacy_sse_event(
    server_id: i64,
    sse_url: &Url,
    state: &Arc<LegacySseSessionState>,
    event: &str,
) -> Result<(), String> {
    let parsed = parse_sse_event(event);
    if parsed.data.trim().is_empty() {
        return Ok(());
    }

    match parsed.event_type.as_deref().unwrap_or("message") {
        "endpoint" => {
            let message_url =
                resolve_legacy_sse_message_url(sse_url, parsed.data.trim(), server_id)
                    .map_err(|error| error.to_string())?;
            state.set_message_url(message_url).await;
            Ok(())
        }
        "message" => {
            let message = serde_json::from_str::<serde_json::Value>(&parsed.data)
                .map_err(|error| format!("failed to parse legacy SSE message payload: {error}"))?;
            state.deliver_response(message).await;
            Ok(())
        }
        _ => Ok(()),
    }
}

#[async_trait]
impl McpSession for LegacySseSession {
    async fn list_tools(&self) -> Result<Vec<McpDiscoveredToolRecord>, McpRuntimeError> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
        let tools = result
            .get("tools")
            .cloned()
            .ok_or(McpRuntimeError::ConnectFailed {
                server_id: self.server_id,
                reason: "legacy SSE tools/list response missing tools".to_string(),
            })?;
        let parsed_tools = serde_json::from_value::<Vec<RmcpTool>>(tools).map_err(|error| {
            McpRuntimeError::Serialization {
                reason: error.to_string(),
            }
        })?;

        parsed_tools
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
        let result = self
            .request(
                "tools/call",
                serde_json::json!({
                    "name": tool_name_original,
                    "arguments": arguments,
                }),
            )
            .await?;
        let call_tool_result = serde_json::from_value::<rmcp::model::CallToolResult>(result)
            .map_err(|error| McpRuntimeError::Serialization {
                reason: error.to_string(),
            })?;

        call_tool_result_to_json(self.server_id, tool_name_original, call_tool_result)
    }
}

fn build_reqwest_client(
    headers: &std::collections::BTreeMap<String, String>,
    server_id: i64,
) -> Result<Client, McpRuntimeError> {
    let parsed_headers = parse_headers(headers, server_id)?;
    let mut default_headers = HeaderMap::new();
    for (header_name, header_value) in parsed_headers {
        default_headers.insert(header_name, header_value);
    }

    Client::builder()
        .default_headers(default_headers)
        .build()
        .map_err(|error| McpRuntimeError::ConnectFailed {
            server_id,
            reason: error.to_string(),
        })
}

fn ensure_legacy_sse_response(response: &Response, server_id: i64) -> Result<(), McpRuntimeError> {
    if !response.status().is_success() {
        return Err(McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!("unexpected SSE response status: {}", response.status()),
        });
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !matches!(content_type, Some(value) if value.contains("text/event-stream")) {
        return Err(McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!(
                "unexpected SSE content type: {}",
                content_type.unwrap_or("<missing>")
            ),
        });
    }

    Ok(())
}

async fn handle_inline_legacy_sse_response(
    state: &Arc<LegacySseSessionState>,
    response: Response,
    server_id: i64,
) -> Result<(), McpRuntimeError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| McpRuntimeError::ConnectFailed {
            server_id,
            reason: error.to_string(),
        })?;
    if bytes.is_empty() {
        return Ok(());
    }

    if let Ok(message) = serde_json::from_slice::<serde_json::Value>(&bytes) {
        state.deliver_response(message).await;
    }

    Ok(())
}

fn extract_json_rpc_result(
    server_id: i64,
    response: serde_json::Value,
) -> Result<serde_json::Value, McpRuntimeError> {
    if let Some(error) = response.get("error") {
        return Err(McpRuntimeError::ConnectFailed {
            server_id,
            reason: format_json_rpc_error(error),
        });
    }

    response
        .get("result")
        .cloned()
        .ok_or(McpRuntimeError::ConnectFailed {
            server_id,
            reason: "legacy SSE response missing result".to_string(),
        })
}

fn format_json_rpc_error(error: &serde_json::Value) -> String {
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown JSON-RPC error");
    let code = error
        .get("code")
        .and_then(serde_json::Value::as_i64)
        .map(|code| format!("code {code}"))
        .unwrap_or_else(|| "unknown code".to_string());
    let data = error
        .get("data")
        .map(serde_json::Value::to_string)
        .filter(|value| value != "null")
        .unwrap_or_default();

    if data.is_empty() {
        format!("{message} ({code})")
    } else {
        format!("{message} ({code}): {data}")
    }
}

fn take_sse_event(buffer: &mut String) -> Option<String> {
    let separator = match (buffer.find("\r\n\r\n"), buffer.find("\n\n")) {
        (Some(crlf_index), Some(lf_index)) if crlf_index <= lf_index => Some((crlf_index, 4)),
        (Some(crlf_index), _) => Some((crlf_index, 4)),
        (_, Some(lf_index)) => Some((lf_index, 2)),
        (None, None) => None,
    }?;

    let (index, separator_len) = separator;
    let event = buffer[..index].to_string();
    buffer.drain(..index + separator_len);
    Some(event)
}

struct ParsedSseEvent {
    event_type: Option<String>,
    data: String,
}

fn parse_sse_event(event: &str) -> ParsedSseEvent {
    let normalized = event.replace("\r\n", "\n");
    let mut event_type = None;
    let mut data_lines = Vec::new();

    for line in normalized.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    ParsedSseEvent {
        event_type,
        data: data_lines.join("\n"),
    }
}

fn resolve_legacy_sse_message_url(
    sse_url: &Url,
    endpoint: &str,
    server_id: i64,
) -> Result<Url, McpRuntimeError> {
    if endpoint.is_empty() {
        return Err(McpRuntimeError::ConnectFailed {
            server_id,
            reason: "legacy SSE endpoint event did not include a message URL".to_string(),
        });
    }

    if let Ok(absolute_url) = Url::parse(endpoint) {
        return Ok(absolute_url);
    }

    sse_url
        .join(endpoint)
        .map_err(|error| McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!("invalid legacy SSE message endpoint '{endpoint}': {error}"),
        })
}

fn parse_streamable_http_payload(
    server_id: i64,
    content_type: &Option<String>,
    bytes: &[u8],
) -> Result<serde_json::Value, McpRuntimeError> {
    let body = String::from_utf8_lossy(bytes).to_string();

    if content_type
        .as_deref()
        .is_some_and(|value| value.starts_with("text/event-stream"))
        || body.contains("\nevent:")
        || body.starts_with("event:")
        || body.starts_with("id:")
    {
        return parse_sse_response_payload(server_id, &body);
    }

    serde_json::from_slice::<serde_json::Value>(bytes).map_err(|error| {
        McpRuntimeError::ConnectFailed {
            server_id,
            reason: format!("failed to parse streamable HTTP response body: {error}"),
        }
    })
}

fn parse_sse_response_payload(
    server_id: i64,
    body: &str,
) -> Result<serde_json::Value, McpRuntimeError> {
    let mut buffer = body.to_string();
    while let Some(event) = take_sse_event(&mut buffer) {
        let parsed = parse_sse_event(&event);
        if parsed.data.trim().is_empty() {
            continue;
        }

        if parsed.event_type.as_deref().unwrap_or("message") != "message" {
            continue;
        }

        return serde_json::from_str::<serde_json::Value>(&parsed.data).map_err(|error| {
            McpRuntimeError::ConnectFailed {
                server_id,
                reason: format!("failed to parse streamable HTTP SSE payload: {error}"),
            }
        });
    }

    Err(McpRuntimeError::ConnectFailed {
        server_id,
        reason: "streamable HTTP SSE response did not contain a JSON-RPC message".to_string(),
    })
}

fn format_http_error(
    status: reqwest::StatusCode,
    content_type: &Option<String>,
    bytes: &[u8],
) -> String {
    let body = String::from_utf8_lossy(bytes);
    let summarized_body = if body.len() > 400 {
        format!("{}...", &body[..400])
    } else {
        body.to_string()
    };

    match content_type {
        Some(content_type) => format!("HTTP {status} [{content_type}]: {summarized_body}"),
        None => format!("HTTP {status}: {summarized_body}"),
    }
}

struct RmcpSession {
    server_id: i64,
    client: tokio::sync::Mutex<RunningService<RoleClient, ClientInfo>>,
}

impl RmcpSession {
    fn new(server_id: i64, client: RunningService<RoleClient, ClientInfo>) -> Self {
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

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Mutex;
    use tokio::sync::mpsc;

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

    #[derive(Default)]
    struct LegacySseTestState {
        event_sender: Mutex<Option<mpsc::UnboundedSender<String>>>,
    }

    impl LegacySseTestState {
        async fn register_stream(&self) -> mpsc::UnboundedReceiver<String> {
            let (sender, receiver) = mpsc::unbounded_channel();
            *self.event_sender.lock().await = Some(sender);
            receiver
        }

        async fn send_message(&self, body: serde_json::Value) {
            let sender = self.event_sender.lock().await.clone();
            if let Some(sender) = sender {
                let _ = sender.send(format!("event: message\ndata: {body}\n\n"));
            }
        }
    }

    struct LegacySseTestServer {
        join_handle: tokio::task::JoinHandle<()>,
        sse_url: String,
    }

    impl LegacySseTestServer {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("test SSE server should bind");
            let address = listener
                .local_addr()
                .expect("listener should expose address");
            let state = Arc::new(LegacySseTestState::default());
            let join_handle = tokio::spawn(run_legacy_sse_test_server(listener, state));
            Self {
                join_handle,
                sse_url: format!("http://{address}/sse"),
            }
        }
    }

    impl Drop for LegacySseTestServer {
        fn drop(&mut self) {
            self.join_handle.abort();
        }
    }

    async fn run_legacy_sse_test_server(listener: TcpListener, state: Arc<LegacySseTestState>) {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let _ = handle_legacy_sse_test_connection(stream, state).await;
            });
        }
    }

    async fn handle_legacy_sse_test_connection(
        stream: TcpStream,
        state: Arc<LegacySseTestState>,
    ) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).await? == 0 {
            return Ok(());
        }

        let mut content_length = 0usize;
        loop {
            let mut header_line = String::new();
            if reader.read_line(&mut header_line).await? == 0 {
                return Ok(());
            }
            if header_line == "\r\n" || header_line == "\n" {
                break;
            }

            if let Some((name, value)) = header_line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = value.trim().parse::<usize>().unwrap_or_default();
            }
        }

        let mut body = vec![0; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).await?;
        }

        let mut stream = reader.into_inner();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default();
        let path = request_parts.next().unwrap_or_default();

        match (method, path) {
            ("GET", "/sse") => handle_legacy_sse_stream(&mut stream, state).await,
            ("POST", "/message?sessionId=test-session") => {
                handle_legacy_sse_message(&mut stream, state, &body).await
            }
            _ => {
                stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found",
                    )
                    .await?;
                Ok(())
            }
        }
    }

    async fn handle_legacy_sse_stream(
        stream: &mut TcpStream,
        state: Arc<LegacySseTestState>,
    ) -> std::io::Result<()> {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\nevent: endpoint\ndata: /message?sessionId=test-session\n\n",
            )
            .await?;
        stream.flush().await?;

        let mut events = state.register_stream().await;
        while let Some(event) = events.recv().await {
            stream.write_all(event.as_bytes()).await?;
            stream.flush().await?;
        }

        Ok(())
    }

    async fn handle_legacy_sse_message(
        stream: &mut TcpStream,
        state: Arc<LegacySseTestState>,
        body: &[u8],
    ) -> std::io::Result<()> {
        let request: serde_json::Value =
            serde_json::from_slice(body).expect("test message body should be valid JSON");
        if let Some(response) = legacy_sse_response(&request) {
            state.send_message(response).await;
        }

        stream
            .write_all(
                b"HTTP/1.1 202 Accepted\r\nContent-Length: 8\r\nConnection: close\r\n\r\nAccepted",
            )
            .await?;
        Ok(())
    }

    fn legacy_sse_response(request: &serde_json::Value) -> Option<serde_json::Value> {
        let method = request.get("method")?.as_str()?;
        match method {
            "initialize" => Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {
                            "listChanged": true
                        }
                    },
                    "serverInfo": {
                        "name": "legacy-sse-test",
                        "version": "1.0.0"
                    }
                }
            })),
            "tools/list" => Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "result": {
                    "tools": [
                        {
                            "name": "echo",
                            "title": "Echo Tool",
                            "description": "Echoes back the input string",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "message": {
                                        "type": "string"
                                    }
                                },
                                "required": ["message"],
                                "additionalProperties": false
                            }
                        }
                    ]
                }
            })),
            "tools/call" => Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": "pong"
                        }
                    ]
                }
            })),
            "notifications/initialized" => None,
            _ => None,
        }
    }

    #[derive(Default)]
    struct StreamableHttpTestState {
        initialize_versions: Mutex<Vec<String>>,
    }

    impl StreamableHttpTestState {
        async fn push_initialize_version(&self, version: String) {
            self.initialize_versions.lock().await.push(version);
        }

        async fn initialize_versions(&self) -> Vec<String> {
            self.initialize_versions.lock().await.clone()
        }
    }

    struct StreamableHttpTestServer {
        join_handle: tokio::task::JoinHandle<()>,
        url: String,
        state: Arc<StreamableHttpTestState>,
    }

    impl StreamableHttpTestServer {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("test HTTP server should bind");
            let address = listener
                .local_addr()
                .expect("listener should expose address");
            let state = Arc::new(StreamableHttpTestState::default());
            let join_handle = tokio::spawn(run_streamable_http_test_server(
                listener,
                Arc::clone(&state),
            ));
            Self {
                join_handle,
                url: format!("http://{address}/mcp"),
                state,
            }
        }

        async fn initialize_versions(&self) -> Vec<String> {
            self.state.initialize_versions().await
        }
    }

    impl Drop for StreamableHttpTestServer {
        fn drop(&mut self) {
            self.join_handle.abort();
        }
    }

    async fn run_streamable_http_test_server(
        listener: TcpListener,
        state: Arc<StreamableHttpTestState>,
    ) {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let _ = handle_streamable_http_test_connection(stream, state).await;
            });
        }
    }

    async fn handle_streamable_http_test_connection(
        stream: TcpStream,
        state: Arc<StreamableHttpTestState>,
    ) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).await? == 0 {
            return Ok(());
        }

        let mut content_length = 0usize;
        let mut headers = HashMap::new();
        loop {
            let mut header_line = String::new();
            if reader.read_line(&mut header_line).await? == 0 {
                return Ok(());
            }
            if header_line == "\r\n" || header_line == "\n" {
                break;
            }

            if let Some((name, value)) = header_line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse::<usize>().unwrap_or_default();
                }
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }

        let mut body = vec![0; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).await?;
        }

        let mut stream = reader.into_inner();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default();
        let path = request_parts.next().unwrap_or_default();

        match (method, path) {
            ("POST", "/mcp") => {
                handle_streamable_http_test_message(&mut stream, state, &headers, &body).await
            }
            ("GET", "/mcp") => {
                stream
                    .write_all(
                        b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await?;
                Ok(())
            }
            _ => {
                stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found",
                    )
                    .await?;
                Ok(())
            }
        }
    }

    async fn handle_streamable_http_test_message(
        stream: &mut TcpStream,
        state: Arc<StreamableHttpTestState>,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> std::io::Result<()> {
        let request: serde_json::Value =
            serde_json::from_slice(body).expect("test message body should be valid JSON");
        let method = request
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let accept = headers.get("accept").cloned().unwrap_or_default();
        let has_expected_accept =
            accept.contains("application/json") && accept.contains("text/event-stream");

        if !has_expected_accept {
            let body = serde_json::json!({
                "message": "Accept header must include both application/json and text/event-stream"
            })
            .to_string();
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await?;
            return Ok(());
        }

        match method {
            "initialize" => {
                let version = request
                    .get("params")
                    .and_then(|value| value.get("protocolVersion"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                state.push_initialize_version(version.clone()).await;

                if version != "2024-11-05" {
                    let payload = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                        "error": {
                            "code": -32602,
                            "message": format!("unsupported protocol version: {version}")
                        }
                    });
                    let body = payload.to_string();
                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            )
                            .as_bytes(),
                        )
                        .await?;
                    return Ok(());
                }

                let payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {
                                "listChanged": true
                            }
                        },
                        "serverInfo": {
                            "name": "streamable-http-test",
                            "version": "1.0.0"
                        }
                    }
                });
                let sse_body = format!(
                    "id:{}\nevent:message\ndata:{}\n\n",
                    request
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or_default(),
                    payload
                );
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n{}: test-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            MCP_SESSION_ID_HEADER,
                            sse_body.len(),
                            sse_body
                        )
                        .as_bytes(),
                    )
                    .await?;
                Ok(())
            }
            "notifications/initialized" => {
                let protocol_version = headers
                    .get(MCP_PROTOCOL_VERSION_HEADER)
                    .cloned()
                    .unwrap_or_default();
                let session_id = headers
                    .get(MCP_SESSION_ID_HEADER)
                    .cloned()
                    .unwrap_or_default();
                if protocol_version != "2024-11-05" || session_id != "test-session" {
                    stream
                        .write_all(
                            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await?;
                    return Ok(());
                }
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await?;
                Ok(())
            }
            "tools/list" => {
                let protocol_version = headers
                    .get(MCP_PROTOCOL_VERSION_HEADER)
                    .cloned()
                    .unwrap_or_default();
                let session_id = headers
                    .get(MCP_SESSION_ID_HEADER)
                    .cloned()
                    .unwrap_or_default();
                if protocol_version != "2024-11-05" || session_id != "test-session" {
                    stream
                        .write_all(
                            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await?;
                    return Ok(());
                }

                let payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                    "result": {
                        "tools": [
                            {
                                "name": "echo",
                                "title": "Echo Tool",
                                "description": "Echoes back the input string",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "message": {
                                            "type": "string"
                                        }
                                    },
                                    "required": ["message"],
                                    "additionalProperties": false
                                }
                            }
                        ]
                    }
                });
                let sse_body = format!(
                    "id:{}\nevent:message\ndata:{}\n\n",
                    request
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or_default(),
                    payload
                );
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n{}: test-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            MCP_SESSION_ID_HEADER,
                            sse_body.len(),
                            sse_body
                        )
                        .as_bytes(),
                    )
                    .await?;
                Ok(())
            }
            "tools/call" => {
                let protocol_version = headers
                    .get(MCP_PROTOCOL_VERSION_HEADER)
                    .cloned()
                    .unwrap_or_default();
                let session_id = headers
                    .get(MCP_SESSION_ID_HEADER)
                    .cloned()
                    .unwrap_or_default();
                if protocol_version != "2024-11-05" || session_id != "test-session" {
                    stream
                        .write_all(
                            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await?;
                    return Ok(());
                }

                let payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": "pong"
                            }
                        ]
                    }
                });
                let sse_body = format!(
                    "id:{}\nevent:message\ndata:{}\n\n",
                    request
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or_default(),
                    payload
                );
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n{}: test-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            MCP_SESSION_ID_HEADER,
                            sse_body.len(),
                            sse_body
                        )
                        .as_bytes(),
                    )
                    .await?;
                Ok(())
            }
            _ => {
                stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found",
                    )
                    .await?;
                Ok(())
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

    #[tokio::test]
    async fn rmcp_connector_supports_legacy_sse_tool_discovery() {
        let server = LegacySseTestServer::start().await;
        let connector = RmcpConnector;
        let session = connector
            .connect(&McpServerRecord {
                id: Some(11),
                display_name: "Legacy SSE".to_string(),
                enabled: true,
                transport: McpTransportConfig::Sse {
                    url: server.sse_url.clone(),
                    headers: Default::default(),
                },
                timeout_ms: 3_000,
                status: McpServerStatus::Failed,
                last_checked_at: None,
                last_success_at: None,
                last_error: None,
                discovered_tool_count: 0,
            })
            .await
            .expect("legacy sse connector should establish a session");

        let tools = session
            .list_tools()
            .await
            .expect("legacy sse session should discover tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_name_original, "echo");
    }

    #[tokio::test]
    async fn legacy_sse_session_supports_tool_calls() {
        let server = LegacySseTestServer::start().await;
        let connector = RmcpConnector;
        let session = connector
            .connect(&McpServerRecord {
                id: Some(12),
                display_name: "Legacy SSE".to_string(),
                enabled: true,
                transport: McpTransportConfig::Sse {
                    url: server.sse_url.clone(),
                    headers: Default::default(),
                },
                timeout_ms: 3_000,
                status: McpServerStatus::Failed,
                last_checked_at: None,
                last_success_at: None,
                last_error: None,
                discovered_tool_count: 0,
            })
            .await
            .expect("legacy sse connector should establish a session");

        let result = session
            .call_tool("echo", serde_json::json!({ "message": "ping" }))
            .await
            .expect("legacy sse session should execute tools");
        assert_eq!(
            result,
            serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": "pong"
                    }
                ]
            })
        );
    }

    #[tokio::test]
    async fn rmcp_connector_falls_back_to_legacy_http_protocol_version() {
        let server = StreamableHttpTestServer::start().await;
        let connector = RmcpConnector;
        let session = connector
            .connect(&McpServerRecord {
                id: Some(13),
                display_name: "Legacy HTTP".to_string(),
                enabled: true,
                transport: McpTransportConfig::Http {
                    url: server.url.clone(),
                    headers: Default::default(),
                },
                timeout_ms: 3_000,
                status: McpServerStatus::Failed,
                last_checked_at: None,
                last_success_at: None,
                last_error: None,
                discovered_tool_count: 0,
            })
            .await
            .expect("http connector should fall back to an older protocol version");

        let tools = session
            .list_tools()
            .await
            .expect("legacy http session should discover tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            server.initialize_versions().await,
            vec![
                "2025-06-18".to_string(),
                "2025-03-26".to_string(),
                "2024-11-05".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn streamable_http_session_supports_tool_calls_with_lenient_notifications() {
        let server = StreamableHttpTestServer::start().await;
        let connector = RmcpConnector;
        let session = connector
            .connect(&McpServerRecord {
                id: Some(14),
                display_name: "Streamable HTTP".to_string(),
                enabled: true,
                transport: McpTransportConfig::Http {
                    url: server.url.clone(),
                    headers: Default::default(),
                },
                timeout_ms: 3_000,
                status: McpServerStatus::Failed,
                last_checked_at: None,
                last_success_at: None,
                last_error: None,
                discovered_tool_count: 0,
            })
            .await
            .expect("http connector should establish a session");

        let result = session
            .call_tool("echo", serde_json::json!({ "message": "ping" }))
            .await
            .expect("streamable http session should execute tools");
        assert_eq!(
            result,
            serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": "pong"
                    }
                ]
            })
        );
    }
}
