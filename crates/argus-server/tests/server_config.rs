use std::net::SocketAddr;
use std::path::PathBuf;

use argus_server::server_config::ServerConfig;

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
    let config = ServerConfig::from_env_values(Some("127.0.0.1:4181"), Some("/opt/arguswing/web"))
        .expect("custom config should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 4181)));
    assert_eq!(
        config.web_dist_dir,
        Some(PathBuf::from("/opt/arguswing/web"))
    );
}
