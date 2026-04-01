use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{AgentId, NamedTool, Result};

#[test]
fn mcp_server_record_round_trips_through_json() {
    let record = McpServerRecord::for_test_stdio("Local Slack");
    let value = serde_json::to_value(&record).unwrap();
    let decoded: McpServerRecord = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, record);
}

#[test]
fn mcp_transport_config_uses_discriminated_json() {
    let stdio = serde_json::to_value(McpTransportConfig::Stdio {
        command: "slack-mcp".into(),
        args: vec!["--stdio".into()],
        env: Default::default(),
    })
    .unwrap();
    assert_eq!(stdio["kind"], "stdio");

    let http = serde_json::to_value(McpTransportConfig::Http {
        url: "https://example.invalid/mcp".into(),
        headers: Default::default(),
    })
    .unwrap();
    assert_eq!(http["kind"], "http");

    let sse = serde_json::to_value(McpTransportConfig::Sse {
        url: "https://example.invalid/sse".into(),
        headers: Default::default(),
    })
    .unwrap();
    assert_eq!(sse["kind"], "sse");
}

#[test]
fn agent_mcp_binding_supports_server_binding_and_tool_whitelist() {
    let server = AgentMcpServerBinding {
        agent_id: AgentId::new(42),
        server_id: 7,
    };

    let unrestricted = AgentMcpBinding {
        server: server.clone(),
        allowed_tools: None,
    };
    assert!(unrestricted.allows_tool("any_tool"));

    let restricted = AgentMcpBinding {
        server,
        allowed_tools: Some(vec!["post_message".into(), "list_channels".into()]),
    };
    assert!(restricted.allows_tool("post_message"));
    assert!(!restricted.allows_tool("delete_channel"));
}

#[test]
fn thread_event_notice_round_trips_through_json() {
    let event = crate::ThreadEvent::Notice {
        thread_id: "thread-123".to_string(),
        level: ThreadNoticeLevel::Info,
        message: "MCP connection restored".to_string(),
    };
    let value = serde_json::to_value(&event).unwrap();
    let decoded: crate::ThreadEvent = serde_json::from_value(value.clone()).unwrap();
    let round_trip = serde_json::to_value(&decoded).unwrap();
    assert_eq!(round_trip, value);
}

#[test]
fn mcp_discovered_tool_record_round_trips_through_json() {
    let record = McpDiscoveredToolRecord {
        server_id: 7,
        tool_name_original: "post_message".to_string(),
        description: "Send a message".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            }
        }),
        annotations: Some(serde_json::json!({ "title": "post_message" })),
    };

    let value = serde_json::to_value(&record).unwrap();
    let decoded: McpDiscoveredToolRecord = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, record);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadNoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportKind {
    Stdio,
    Http,
    Sse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransportConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        env: std::collections::BTreeMap<String, String>,
    },
    Http {
        url: String,
        headers: std::collections::BTreeMap<String, String>,
    },
    Sse {
        url: String,
        headers: std::collections::BTreeMap<String, String>,
    },
}

impl McpTransportConfig {
    #[must_use]
    pub fn kind(&self) -> McpTransportKind {
        match self {
            Self::Stdio { .. } => McpTransportKind::Stdio,
            Self::Http { .. } => McpTransportKind::Http,
            Self::Sse { .. } => McpTransportKind::Sse,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerRecord {
    pub id: Option<i64>,
    pub display_name: String,
    pub enabled: bool,
    pub transport: McpTransportConfig,
    pub timeout_ms: u64,
    pub status: McpServerStatus,
    pub last_checked_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub discovered_tool_count: u32,
}

impl McpServerRecord {
    #[cfg(test)]
    #[must_use]
    pub fn for_test_stdio(display_name: &str) -> Self {
        Self {
            id: Some(1),
            display_name: display_name.to_string(),
            enabled: true,
            transport: McpTransportConfig::Stdio {
                command: "slack-mcp".to_string(),
                args: vec!["--stdio".to_string()],
                env: std::collections::BTreeMap::new(),
            },
            timeout_ms: 30_000,
            status: McpServerStatus::Ready,
            last_checked_at: Some("2026-04-01T00:00:00Z".to_string()),
            last_success_at: Some("2026-04-01T00:00:00Z".to_string()),
            last_error: None,
            discovered_tool_count: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerStatus {
    Ready,
    Connecting,
    Retrying,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpDiscoveredToolRecord {
    pub server_id: i64,
    pub tool_name_original: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub annotations: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMcpServerBinding {
    pub agent_id: AgentId,
    pub server_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMcpToolBinding {
    pub agent_id: AgentId,
    pub server_id: i64,
    pub tool_name_original: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMcpBinding {
    pub server: AgentMcpServerBinding,
    pub allowed_tools: Option<Vec<String>>,
}

impl AgentMcpBinding {
    #[must_use]
    pub fn allows_tool(&self, tool_name_original: &str) -> bool {
        self.allowed_tools
            .as_ref()
            .map(|allowed| allowed.iter().any(|tool| tool == tool_name_original))
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpUnavailableServerSummary {
    pub server_id: i64,
    pub display_name: String,
    pub reason: String,
}

pub struct ResolvedMcpTools {
    pub tools: Vec<Arc<dyn NamedTool>>,
    pub unavailable_servers: Vec<McpUnavailableServerSummary>,
}

impl ResolvedMcpTools {
    #[must_use]
    pub fn new(
        tools: Vec<Arc<dyn NamedTool>>,
        unavailable_servers: Vec<McpUnavailableServerSummary>,
    ) -> Self {
        Self {
            tools,
            unavailable_servers,
        }
    }
}

impl Default for ResolvedMcpTools {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            unavailable_servers: Vec::new(),
        }
    }
}

#[async_trait]
pub trait McpToolResolver: Send + Sync {
    async fn resolve_for_agent(&self, agent_id: AgentId) -> Result<ResolvedMcpTools>;
}
