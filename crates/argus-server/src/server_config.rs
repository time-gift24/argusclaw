use std::net::SocketAddr;
use std::path::PathBuf;

use argus_thread_pool::{ThreadPoolConfig, ThreadPoolConfigError};
use thiserror::Error;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";

#[derive(Debug, Error)]
pub enum ServerConfigError {
    #[error("invalid bind address: {0}")]
    BindAddr(#[from] std::net::AddrParseError),
    #[error("invalid thread pool config: {0}")]
    ThreadPool(#[from] ThreadPoolConfigError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub web_dist_dir: Option<PathBuf>,
    pub thread_pool: ThreadPoolConfig,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ServerConfigError> {
        Self::from_env_values(
            std::env::var("ARGUS_SERVER_ADDR").ok().as_deref(),
            std::env::var("ARGUS_WEB_DIST_DIR").ok().as_deref(),
            std::env::var("ARGUS_THREAD_POOL_MAX_THREADS")
                .ok()
                .as_deref(),
            std::env::var("ARGUS_THREAD_POOL_MAX_ESTIMATED_MEMORY_BYTES")
                .ok()
                .as_deref(),
        )
    }

    pub fn from_env_value(value: Option<&str>) -> Result<Self, ServerConfigError> {
        Self::from_env_values(value, None, None, None)
    }

    pub fn from_env_values(
        bind_addr: Option<&str>,
        web_dist_dir: Option<&str>,
        thread_pool_max_threads: Option<&str>,
        thread_pool_max_estimated_memory_bytes: Option<&str>,
    ) -> Result<Self, ServerConfigError> {
        let bind_addr = bind_addr.unwrap_or(DEFAULT_BIND_ADDR).parse()?;
        let web_dist_dir = web_dist_dir.map(PathBuf::from);
        let thread_pool = ThreadPoolConfig::from_env_values(
            thread_pool_max_threads,
            thread_pool_max_estimated_memory_bytes,
        )?;
        Ok(Self {
            bind_addr,
            web_dist_dir,
            thread_pool,
        })
    }
}
