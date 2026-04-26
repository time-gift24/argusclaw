#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = argus_server::server_config::ServerConfig::from_env()?;
    let app = argus_server::build_app_with_config(None, &config).await?;
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
