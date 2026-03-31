use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::Json;
use argus_protocol::{
    LlmProviderId, LlmProviderRecord, LlmProviderRecordJson, ProviderSecretStatus, SecretString,
};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/providers", get(list_providers).post(upsert_provider))
        .route("/providers/default", get(get_default_provider_record))
        .route(
            "/providers/test-input",
            post(test_provider_input),
        )
        .route(
            "/providers/{id}",
            get(get_provider_record).delete(delete_provider),
        )
        .route("/providers/{id}/default", put(set_default_provider))
        .route("/providers/{id}/context-window", get(get_context_window))
        .route("/providers/{id}/test", post(test_provider_connection))
}

async fn list_providers(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let providers = state.wing.list_providers().await.map_err(ApiError::from)?;
    let json_list: Vec<LlmProviderRecordJson> =
        providers.into_iter().map(Into::into).collect();
    Ok(Json(json_list))
}

async fn get_default_provider_record(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let record = state
        .wing
        .get_default_provider_record()
        .await
        .map_err(ApiError::from)?;
    let json: LlmProviderRecordJson = if record.secret_status == ProviderSecretStatus::RequiresReentry {
        build_provider_reentry_record(record)
    } else {
        record.into()
    };
    Ok(Json(json))
}

async fn get_provider_record(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let provider_id = LlmProviderId::new(id);
    match state.wing.get_provider_record(provider_id).await {
        Ok(record) => {
            let json: LlmProviderRecordJson =
                if record.secret_status == ProviderSecretStatus::RequiresReentry {
                    build_provider_reentry_record(record)
                } else {
                    record.into()
                };
            Ok(Json(json))
        }
        Err(argus_protocol::ArgusError::ProviderNotFound(_)) => Err(ApiError::NotFound(
            format!("Provider {id} not found"),
        )),
        Err(e) => Err(ApiError::from(e)),
    }
}

async fn get_context_window(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let provider_id = LlmProviderId::new(id);
    let context_window = match state.wing.get_provider(provider_id).await {
        Ok(provider) => provider.context_window(),
        Err(_) => 128_000,
    };
    Ok(Json(serde_json::json!({ "context_window": context_window })))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestConnectionBody {
    pub model: String,
}

async fn test_provider_connection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<TestConnectionBody>,
) -> Result<impl IntoResponse, ApiError> {
    let result = state
        .wing
        .test_provider_connection(LlmProviderId::new(id), &body.model)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestInputBody {
    pub record: LlmProviderRecordJson,
    pub model: String,
}

async fn test_provider_input(
    State(state): State<AppState>,
    Json(body): Json<TestInputBody>,
) -> Result<impl IntoResponse, ApiError> {
    let record = LlmProviderRecord {
        id: LlmProviderId::new(body.record.id),
        kind: body.record.kind,
        display_name: body.record.display_name,
        base_url: body.record.base_url,
        api_key: SecretString::new(body.record.api_key),
        models: body.record.models,
        model_config: body.record.model_config,
        default_model: body.record.default_model,
        is_default: body.record.is_default,
        extra_headers: body.record.extra_headers,
        secret_status: body.record.secret_status,
        meta_data: body.record.meta_data,
    };
    let result = state
        .wing
        .test_provider_record(record, &body.model)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn upsert_provider(
    State(state): State<AppState>,
    Json(body): Json<LlmProviderRecordJson>,
) -> Result<impl IntoResponse, ApiError> {
    let record = LlmProviderRecord {
        id: LlmProviderId::new(body.id),
        kind: body.kind,
        display_name: body.display_name,
        base_url: body.base_url,
        api_key: SecretString::new(body.api_key),
        models: body.models,
        model_config: body.model_config,
        default_model: body.default_model,
        is_default: body.is_default,
        extra_headers: body.extra_headers,
        secret_status: body.secret_status,
        meta_data: body.meta_data,
    };
    let id = state.wing.upsert_provider(record).await.map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "id": id.to_string() })))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .delete_provider(LlmProviderId::new(id))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn set_default_provider(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .set_default_provider(LlmProviderId::new(id))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

fn build_provider_reentry_record(record: LlmProviderRecord) -> LlmProviderRecordJson {
    LlmProviderRecordJson {
        id: record.id.into_inner(),
        kind: record.kind,
        display_name: record.display_name,
        base_url: record.base_url,
        api_key: String::new(),
        models: record.models,
        model_config: record.model_config,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
        meta_data: record.meta_data,
    }
}
