use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use argus_protocol::{AgentId, AgentMcpBinding, AgentMcpServerBinding, AgentRecord};
use argus_repository::types::AgentDeleteReport;
use argus_template::TemplateDeleteOptions;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::MutationResponse;
use crate::user_context::RequestUser;

use super::require_admin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMcpBindingPayload {
    pub server_id: i64,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
}

impl AgentMcpBindingPayload {
    fn from_binding(binding: AgentMcpBinding) -> Self {
        Self {
            server_id: binding.server.server_id,
            allowed_tools: binding.allowed_tools,
        }
    }

    fn into_binding(self, agent_id: AgentId) -> AgentMcpBinding {
        AgentMcpBinding {
            server: AgentMcpServerBinding {
                agent_id,
                server_id: self.server_id,
            },
            allowed_tools: self.allowed_tools,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateRecordPayload {
    #[serde(flatten)]
    pub record: AgentRecord,
    #[serde(default)]
    pub mcp_bindings: Vec<AgentMcpBindingPayload>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteTemplateQuery {
    #[serde(default)]
    pub cascade_associations: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateDeleteResponse {
    pub deleted: bool,
    pub agent_deleted: bool,
    pub deleted_job_count: u64,
    pub deleted_run_count: u64,
    pub deleted_thread_count: u64,
    pub deleted_session_count: u64,
}

impl From<AgentDeleteReport> for TemplateDeleteResponse {
    fn from(report: AgentDeleteReport) -> Self {
        Self {
            deleted: report.agent_deleted,
            agent_deleted: report.agent_deleted,
            deleted_job_count: report.deleted_job_count,
            deleted_run_count: report.deleted_run_count,
            deleted_thread_count: report.deleted_thread_count,
            deleted_session_count: report.deleted_session_count,
        }
    }
}

impl TemplateRecordPayload {
    async fn from_record(state: &AppState, record: AgentRecord) -> Result<Self, ApiError> {
        let bindings = state.core().list_agent_mcp_bindings(record.id).await?;
        Ok(Self {
            record,
            mcp_bindings: bindings
                .into_iter()
                .map(AgentMcpBindingPayload::from_binding)
                .collect(),
        })
    }
}

pub async fn list_templates(
    request_user: RequestUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TemplateRecordPayload>>, ApiError> {
    require_admin(&state, &request_user).await?;
    let templates = state.core().list_templates().await?;
    let mut payloads = Vec::with_capacity(templates.len());
    for record in templates {
        payloads.push(TemplateRecordPayload::from_record(&state, record).await?);
    }
    Ok(Json(payloads))
}

pub async fn create_template(
    request_user: RequestUser,
    State(state): State<AppState>,
    Json(payload): Json<TemplateRecordPayload>,
) -> Result<(StatusCode, Json<MutationResponse<TemplateRecordPayload>>), ApiError> {
    require_admin(&state, &request_user).await?;
    let TemplateRecordPayload {
        mut record,
        mcp_bindings,
    } = payload;
    record.id = AgentId::new(0);
    let id = state.core().upsert_template(record).await?;
    let bindings = mcp_bindings
        .into_iter()
        .map(|binding| binding.into_binding(id))
        .collect();
    state.core().set_agent_mcp_bindings(id, bindings).await?;
    let saved = state
        .core()
        .get_template(id)
        .await?
        .ok_or_else(|| ApiError::internal(format!("Template not found after upsert: {id}")))?;
    let payload = TemplateRecordPayload::from_record(&state, saved).await?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(payload))))
}

pub async fn update_template(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(template_id): Path<i64>,
    Json(payload): Json<TemplateRecordPayload>,
) -> Result<Json<MutationResponse<TemplateRecordPayload>>, ApiError> {
    require_admin(&state, &request_user).await?;
    let TemplateRecordPayload {
        mut record,
        mcp_bindings,
    } = payload;
    record.id = AgentId::new(template_id);
    let id = state.core().upsert_template(record).await?;
    let bindings = mcp_bindings
        .into_iter()
        .map(|binding| binding.into_binding(id))
        .collect();
    state.core().set_agent_mcp_bindings(id, bindings).await?;
    let saved = state
        .core()
        .get_template(id)
        .await?
        .ok_or_else(|| ApiError::internal(format!("Template not found after upsert: {id}")))?;
    let payload = TemplateRecordPayload::from_record(&state, saved).await?;
    Ok(Json(MutationResponse::new(payload)))
}

pub async fn delete_template(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(template_id): Path<i64>,
    Query(query): Query<DeleteTemplateQuery>,
) -> Result<Json<MutationResponse<TemplateDeleteResponse>>, ApiError> {
    require_admin(&state, &request_user).await?;
    let report = state
        .core()
        .delete_template_with_options(
            AgentId::new(template_id),
            TemplateDeleteOptions {
                cascade_associations: query.cascade_associations,
            },
        )
        .await?;
    Ok(Json(MutationResponse::new(report.into())))
}
