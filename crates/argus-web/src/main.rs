use argus_wing::ArgusWing;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use argus_web::{build_router, AppState};

#[derive(Parser)]
#[command(name = "argus-web", about = "Web server frontend for ArgusWing")]
struct Args {
    /// Address to bind to
    #[arg(long, default_value = "0.0.0.0:8080")]
    addr: String,

    /// Database path (defaults to ~/.arguswing/sqlite.db)
    #[arg(long)]
    database_path: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("argus_web=info,argus_wing=info")),
        )
        .init();

    let args = Args::parse();

    let wing = ArgusWing::init(args.database_path.as_deref())
        .await
        .expect("Failed to initialize ArgusWing");
    wing.register_default_tools()
        .await
        .expect("Failed to register default tools");

    let state = AppState::new(wing);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&args.addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind to {}: {}", args.addr, e));

    tracing::info!("Listening on {}", args.addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("Shutting down...");
}
