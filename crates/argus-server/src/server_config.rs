use std::net::SocketAddr;
use std::path::PathBuf;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub web_dist_dir: Option<PathBuf>,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, std::net::AddrParseError> {
        Self::from_env_values(
            std::env::var("ARGUS_SERVER_ADDR").ok().as_deref(),
            std::env::var("ARGUS_WEB_DIST_DIR").ok().as_deref(),
        )
    }

    pub fn from_env_value(value: Option<&str>) -> Result<Self, std::net::AddrParseError> {
        Self::from_env_values(value, None)
    }

    pub fn from_env_values(
        bind_addr: Option<&str>,
        web_dist_dir: Option<&str>,
    ) -> Result<Self, std::net::AddrParseError> {
        let bind_addr = bind_addr.unwrap_or(DEFAULT_BIND_ADDR).parse()?;
        let web_dist_dir = web_dist_dir.map(PathBuf::from);
        Ok(Self {
            bind_addr,
            web_dist_dir,
        })
    }
}
