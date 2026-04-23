#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = argus_server::build_app(None).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
