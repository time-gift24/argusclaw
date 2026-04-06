//! Server configuration.

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to listen on (e.g., "0.0.0.0:3000").
    pub listen_addr: String,
    /// Database URL.
    pub database_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:3000".to_string(),
            database_url: String::new(),
        }
    }
}
