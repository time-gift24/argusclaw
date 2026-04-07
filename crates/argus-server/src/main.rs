//! Argus server entry point.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = argus_server::config::ServerConfig::from_env();
    let listen_addr = config.listen_addr.clone();
    let state = argus_server::build_state(config).await?;
    let app = argus_server::build_router(state);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
