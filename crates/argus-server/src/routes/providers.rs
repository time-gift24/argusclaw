use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use argus_protocol::{
    LlmProviderId, LlmProviderRecord, LlmProviderRecordJson, ProviderTestResult, SecretString,
};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConnectionTestRequest {
    pub model: Option<String>,
}

pub async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<LlmProviderRecordJson>>, ApiError> {
    let providers = state.core().list_providers().await?;
    Ok(Json(providers.into_iter().map(Into::into).collect()))
}

pub async fn create_provider(
    State(state): State<AppState>,
    Json(mut record): Json<LlmProviderRecordJson>,
) -> Result<(StatusCode, Json<MutationResponse<LlmProviderRecordJson>>), ApiError> {
    record.id = 0;
    let id = state.core().upsert_provider(from_json(record)).await?;
    let saved = state.core().get_provider_record(id).await?;
    Ok((
        StatusCode::CREATED,
        Json(MutationResponse::new(saved.into())),
    ))
}

pub async fn update_provider(
    State(state): State<AppState>,
    Path(provider_id): Path<i64>,
    Json(mut record): Json<LlmProviderRecordJson>,
) -> Result<Json<MutationResponse<LlmProviderRecordJson>>, ApiError> {
    record.id = provider_id;
    let id = state.core().upsert_provider(from_json(record)).await?;
    let saved = state.core().get_provider_record(id).await?;
    Ok(Json(MutationResponse::new(saved.into())))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Path(provider_id): Path<i64>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    let deleted = state
        .core()
        .delete_provider(LlmProviderId::new(provider_id))
        .await?;
    Ok(Json(MutationResponse::new(DeleteResponse { deleted })))
}

pub async fn test_provider_connection(
    State(state): State<AppState>,
    Path(provider_id): Path<i64>,
    payload: Option<Json<ProviderConnectionTestRequest>>,
) -> Result<Json<ProviderTestResult>, ApiError> {
    let model = payload.and_then(|Json(request)| request.model);
    Ok(Json(
        state
            .core()
            .test_provider_connection(LlmProviderId::new(provider_id), model)
            .await?,
    ))
}

pub async fn test_provider_record(
    State(state): State<AppState>,
    Json(record): Json<LlmProviderRecordJson>,
) -> Result<Json<ProviderTestResult>, ApiError> {
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
