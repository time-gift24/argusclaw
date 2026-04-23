use std::net::SocketAddr;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, std::net::AddrParseError> {
        Self::from_env_value(std::env::var("ARGUS_SERVER_ADDR").ok().as_deref())
    }

    pub fn from_env_value(value: Option<&str>) -> Result<Self, std::net::AddrParseError> {
        let bind_addr = value.unwrap_or(DEFAULT_BIND_ADDR).parse()?;
        Ok(Self { bind_addr })
    }
}
