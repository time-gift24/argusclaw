use std::net::SocketAddr;
use std::path::PathBuf;

use argus_server::server_config::ServerConfig;
use argus_thread_pool::ThreadPoolConfig;

#[test]
fn server_config_defaults_to_localhost_3000() {
    let config = ServerConfig::from_env_value(None).expect("default bind address should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 3000)));
}

#[test]
fn server_config_accepts_argus_server_addr() {
    let config =
        ServerConfig::from_env_value(Some("127.0.0.1:4181")).expect("custom addr should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 4181)));
}

#[test]
fn server_config_accepts_web_dist_dir() {
    let config = ServerConfig::from_env_values(
        Some("127.0.0.1:4181"),
        Some("/opt/arguswing/web"),
        None,
        None,
    )
    .expect("custom config should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 4181)));
    assert_eq!(
        config.web_dist_dir,
        Some(PathBuf::from("/opt/arguswing/web"))
    );
}

#[test]
fn server_config_accepts_thread_pool_env_values() {
    let config = ServerConfig::from_env_values(
        Some("127.0.0.1:4181"),
        Some("/opt/arguswing/web"),
        Some("96"),
        Some("2147483648"),
    )
    .expect("custom config should parse");

    assert_eq!(
        config.thread_pool,
        ThreadPoolConfig {
            max_threads: 96,
            max_estimated_memory_bytes: Some(2 * 1024 * 1024 * 1024),
        }
    );
}
