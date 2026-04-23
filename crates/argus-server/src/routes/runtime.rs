use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream;
use serde::{Deserialize, Serialize};

use argus_protocol::{JobRuntimeState, ThreadPoolState};

use crate::app_state::AppState;
use crate::error::ApiError;

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeStateResponse {
    pub thread_pool: ThreadPoolState,
    pub job_runtime: JobRuntimeState,
}

impl RuntimeStateResponse {
    fn from_state(state: &AppState) -> Self {
        Self {
            thread_pool: state.core().thread_pool_state(),
            job_runtime: state.core().job_runtime_state(),
        }
    }
}

pub async fn get_runtime_state(
    State(state): State<AppState>,
) -> Result<Json<RuntimeStateResponse>, ApiError> {
    Ok(Json(RuntimeStateResponse::from_state(&state)))
}

pub async fn runtime_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.tick().await;
    let stream = stream::unfold(
        (state, interval, true),
        |(state, mut interval, is_initial)| async move {
            if !is_initial {
                interval.tick().await;
            }
            let payload = RuntimeStateResponse::from_state(&state);
            let event = match Event::default()
                .event("runtime.snapshot")
                .json_data(payload)
            {
                Ok(event) => event,
                Err(error) => Event::default()
                    .event("runtime.error")
                    .data(error.to_string()),
            };

            Some((Ok(event), (state, interval, false)))
        },
    );

    Sse::new(stream).keep_alive(KeepAlive::default())
}
