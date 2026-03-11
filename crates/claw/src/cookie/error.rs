use thiserror::Error;

#[derive(Error, Debug)]
pub enum CookieError {
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("CDP command failed: {0}")]
    CdpFailed(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Connection timeout")]
    Timeout,

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type CookieResult<T> = Result<T, CookieError>;