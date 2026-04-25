use std::net::SocketAddr;

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
