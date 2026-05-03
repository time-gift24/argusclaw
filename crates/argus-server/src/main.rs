use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn init_tracing() {
    let rust_log = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::registry()
        .with(EnvFilter::new(rust_log))
        .with(fmt::layer().with_target(true))
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let config = argus_server::server_config::ServerConfig::from_env()?;
    let app = argus_server::build_app_with_config(None, &config).await?;
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
