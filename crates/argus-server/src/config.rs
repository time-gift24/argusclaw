//! Server configuration.

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen_addr: String,
    pub database_url: String,
    pub session_secret: String,
    pub secure_cookies: bool,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let secure_cookies = std::env::var("ARGUS_SERVER_SECURE_COOKIES")
            .ok()
            .and_then(|value| value.parse::<bool>().ok())
            .unwrap_or(true);

        Self {
            listen_addr: std::env::var("ARGUS_SERVER_LISTEN_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:3000".to_string()),
            database_url: std::env::var("DATABASE_URL").unwrap_or_default(),
            session_secret: std::env::var("ARGUS_SERVER_SESSION_SECRET")
                .unwrap_or_else(|_| "dev-only-session-secret".to_string()),
            secure_cookies,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        let mut config = Self::from_env();
        config.secure_cookies = false;
        config
    }
}
