use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use argus_protocol::llm::ModelConfig;
use argus_protocol::{
    LlmProviderId, LlmProviderRecord, LlmProviderRecordJson, ProviderTestResult, SecretString,
};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};
use crate::user_context::RequestUser;

use super::require_admin;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConnectionTestRequest {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderUpdateRequest {
    pub id: i64,
    pub kind: argus_protocol::LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
    pub model_config: std::collections::HashMap<String, ModelConfig>,
    pub default_model: String,
    pub is_default: bool,
    pub extra_headers: std::collections::HashMap<String, String>,
    pub secret_status: argus_protocol::ProviderSecretStatus,
    pub meta_data: std::collections::HashMap<String, String>,
}

pub async fn list_providers(
    request_user: RequestUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<LlmProviderRecordJson>>, ApiError> {
    require_admin(&state, &request_user).await?;
    let providers = state.core().list_providers().await?;
    Ok(Json(providers.into_iter().map(Into::into).collect()))
}

pub async fn create_provider(
    request_user: RequestUser,
    State(state): State<AppState>,
    Json(mut record): Json<LlmProviderRecordJson>,
) -> Result<(StatusCode, Json<MutationResponse<LlmProviderRecordJson>>), ApiError> {
    require_admin(&state, &request_user).await?;
    record.id = 0;
    let id = state.core().upsert_provider(from_json(record)).await?;
    let saved = state.core().get_provider_record(id).await?;
    Ok((
        StatusCode::CREATED,
        Json(MutationResponse::new(saved.into())),
    ))
}

pub async fn update_provider(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(provider_id): Path<i64>,
    Json(mut record): Json<ProviderUpdateRequest>,
) -> Result<Json<MutationResponse<LlmProviderRecordJson>>, ApiError> {
    require_admin(&state, &request_user).await?;
    record.id = provider_id;
    let existing = state
        .core()
        .get_provider_record(LlmProviderId::new(provider_id))
        .await?;
    let id = state
        .core()
        .upsert_provider(from_update_json(record, existing.api_key))
        .await?;
    let saved = state.core().get_provider_record(id).await?;
    Ok(Json(MutationResponse::new(saved.into())))
}

pub async fn delete_provider(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(provider_id): Path<i64>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    require_admin(&state, &request_user).await?;
    let deleted = state
        .core()
        .delete_provider(LlmProviderId::new(provider_id))
        .await?;
    Ok(Json(MutationResponse::new(DeleteResponse { deleted })))
}

pub async fn test_provider_connection(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(provider_id): Path<i64>,
    payload: Option<Json<ProviderConnectionTestRequest>>,
) -> Result<Json<ProviderTestResult>, ApiError> {
    require_admin(&state, &request_user).await?;
    let model = payload.and_then(|Json(request)| request.model);
    Ok(Json(
        state
            .core()
            .test_provider_connection(LlmProviderId::new(provider_id), model)
            .await?,
    ))
}

pub async fn test_provider_record(
    request_user: RequestUser,
    State(state): State<AppState>,
    Json(record): Json<LlmProviderRecordJson>,
) -> Result<Json<ProviderTestResult>, ApiError> {
    require_admin(&state, &request_user).await?;
    Ok(Json(
        state
            .core()
            .test_provider_record(from_json(record), None)
            .await?,
    ))
}

fn from_json(record: LlmProviderRecordJson) -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(record.id),
        kind: record.kind,
        display_name: record.display_name,
        base_url: record.base_url,
        api_key: SecretString::new(record.api_key),
        models: record.models,
        model_config: record.model_config,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
        meta_data: record.meta_data,
    }
}

fn from_update_json(
    record: ProviderUpdateRequest,
    existing_api_key: SecretString,
) -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(record.id),
        kind: record.kind,
        display_name: record.display_name,
        base_url: record.base_url,
        api_key: record
            .api_key
            .map(SecretString::new)
            .unwrap_or(existing_api_key),
        models: record.models,
        model_config: record.model_config,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
        meta_data: record.meta_data,
    }
}
