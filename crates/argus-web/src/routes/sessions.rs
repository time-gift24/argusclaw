use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::Json;
use argus_protocol::{
    AgentId, ChatMessage, ProviderId, Role, SessionId, ThreadId, ToolCall,
};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{session_id}", delete(delete_session))
        .route("/sessions/{session_id}/rename", put(rename_session))
        .route(
            "/sessions/{session_id}/threads",
            get(list_threads).post(create_thread),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}",
            delete(delete_thread),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}/rename",
            put(rename_thread),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}/model",
            put(update_thread_model),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}/activate",
            post(activate_thread),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}/snapshot",
            get(get_thread_snapshot),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}/messages",
            get(get_thread_messages),
        )
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatSessionPayload {
    pub session_id: String,
    pub thread_id: String,
    pub template_id: i64,
    pub effective_provider_id: Option<i64>,
    pub effective_model: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSummaryPayload {
    pub id: String,
    pub name: String,
    pub thread_count: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThreadSummaryPayload {
    pub thread_id: String,
    pub title: Option<String>,
    pub turn_count: i64,
    pub token_count: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThreadSnapshotPayload {
    pub session_id: String,
    pub thread_id: String,
    pub messages: Vec<ChatMessagePayload>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessagePayload {
    pub role: Role,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl From<&ChatMessage> for ChatMessagePayload {
    fn from(m: &ChatMessage) -> Self {
        Self {
            role: m.role,
            content: m.content.clone(),
            reasoning_content: m.reasoning_content.clone(),
            tool_call_id: m.tool_call_id.clone(),
            name: m.name.clone(),
            tool_calls: m.tool_calls.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateSessionBody {
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenameSessionBody {
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateThreadBody {
    pub template_id: i64,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenameThreadBody {
    pub title: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateThreadModelBody {
    pub provider_id: i64,
    pub model: String,
}

// ---------------------------------------------------------------------------
// Session handlers
// ---------------------------------------------------------------------------

async fn list_sessions(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let sessions = state.wing.list_sessions().await.map_err(ApiError::from)?;
    let payload: Vec<SessionSummaryPayload> = sessions
        .into_iter()
        .map(|s| SessionSummaryPayload {
            id: s.id.to_string(),
            name: s.name,
            thread_count: s.thread_count,
            updated_at: s.updated_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(payload))
}

async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id = state
        .wing
        .create_session(&body.name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "session_id": session_id.to_string() })))
}

async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .wing
        .delete_session(session_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn rename_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<RenameSessionBody>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .wing
        .rename_session(session_id, body.name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

// ---------------------------------------------------------------------------
// Thread handlers
// ---------------------------------------------------------------------------

async fn list_threads(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let threads = state.wing.list_threads(session_id).await.map_err(ApiError::from)?;
    let payload: Vec<ThreadSummaryPayload> = threads
        .into_iter()
        .map(|t| ThreadSummaryPayload {
            thread_id: t.id.to_string(),
            title: t.title,
            turn_count: t.turn_count,
            token_count: t.token_count,
            updated_at: t.updated_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(payload))
}

async fn create_thread(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<CreateThreadBody>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let provider_id = body.provider_id.map(ProviderId::new);

    let thread_id = state
        .wing
        .create_thread(session_id, AgentId::new(body.template_id), provider_id, body.model.as_deref())
        .await
        .map_err(ApiError::from)?;

    let (effective_template_id, effective_provider_id, effective_model) = state
        .wing
        .activate_thread(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;

    let payload = ChatSessionPayload {
        session_id: session_id.to_string(),
        thread_id: thread_id.to_string(),
        template_id: effective_template_id.into_inner(),
        effective_provider_id: effective_provider_id.map(|id| id.inner()),
        effective_model,
    };
    Ok(Json(payload))
}

async fn delete_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .wing
        .delete_thread(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn rename_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(body): Json<RenameThreadBody>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .wing
        .rename_thread(session_id, thread_id, body.title)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn update_thread_model(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(body): Json<UpdateThreadModelBody>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let provider_id = ProviderId::new(body.provider_id);

    state
        .wing
        .update_thread_model(session_id, thread_id, provider_id, &body.model)
        .await
        .map_err(ApiError::from)?;

    let (template_id, effective_provider_id, effective_model) = state
        .wing
        .activate_thread(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;

    let payload = ChatSessionPayload {
        session_id: session_id.to_string(),
        thread_id: thread_id.to_string(),
        template_id: template_id.into_inner(),
        effective_provider_id: effective_provider_id.map(|id| id.inner()),
        effective_model,
    };
    Ok(Json(payload))
}

async fn activate_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let (template_id, effective_provider_id, effective_model) = state
        .wing
        .activate_thread(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;

    let payload = ChatSessionPayload {
        session_id: session_id.to_string(),
        thread_id: thread_id.to_string(),
        template_id: template_id.into_inner(),
        effective_provider_id: effective_provider_id.map(|id| id.inner()),
        effective_model,
    };
    Ok(Json(payload))
}

async fn get_thread_snapshot(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let (messages, turn_count, token_count, plan_item_count) = state
        .wing
        .get_thread_snapshot(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;

    let payload = ThreadSnapshotPayload {
        session_id: session_id.to_string(),
        thread_id: thread_id.to_string(),
        messages: messages.iter().map(ChatMessagePayload::from).collect(),
        turn_count,
        token_count,
        plan_item_count: plan_item_count as usize,
    };
    Ok(Json(payload))
}

async fn get_thread_messages(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let messages = state
        .wing
        .get_thread_messages(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;

    let payload: Vec<ChatMessagePayload> =
        messages.iter().map(ChatMessagePayload::from).collect();
    Ok(Json(payload))
}
