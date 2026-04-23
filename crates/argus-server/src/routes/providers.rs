use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use argus_protocol::{LlmProviderId, LlmProviderRecord, LlmProviderRecordJson, SecretString};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::MutationResponse;

pub async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<LlmProviderRecordJson>>, ApiError> {
    let providers = state.wing().list_providers().await?;
    Ok(Json(providers.into_iter().map(Into::into).collect()))
}

pub async fn create_provider(
    State(state): State<AppState>,
    Json(mut record): Json<LlmProviderRecordJson>,
) -> Result<(StatusCode, Json<MutationResponse<LlmProviderRecordJson>>), ApiError> {
    record.id = 0;
    let id = state.wing().upsert_provider(from_json(record)).await?;
    let saved = state.wing().get_provider_record(id).await?;
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
    let id = state.wing().upsert_provider(from_json(record)).await?;
    let saved = state.wing().get_provider_record(id).await?;
    Ok(Json(MutationResponse::new(saved.into())))
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
