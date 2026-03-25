//! Initialization utilities for ArgusWing.

/// Initialize tracing subscriber with file and console logging.
///
/// This sets up logging to:
/// - Console (stdout)
/// - File at `./tmp/arguswing.log`
///
/// The log level can be controlled via the `RUST_LOG` environment variable.
/// For example: `RUST_LOG=debug` or `RUST_LOG=arguswing=debug,argus=info`
///
/// Note: This function is safe to call multiple times - it will only initialize
/// tracing once (subsequent calls will be no-ops).
pub fn init_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let log_dir = std::path::Path::new("./tmp");
        if let Err(e) = std::fs::create_dir_all(log_dir) {
            eprintln!("Failed to create log directory: {}", e);
            return;
        }

        let file_appender = tracing_appender::rolling::never(log_dir, "arguswing.log");

        let env_filter =
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("arguswing=debug,argus=debug,argus_llm=debug")
            });

        let result = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(file_appender)
            .with_ansi(false)
            .try_init();

        match result {
            Ok(()) => {
                println!("Tracing initialized. Logs will be written to ./tmp/arguswing.log");
            }
            Err(_) => {
                // Tracing already initialized by another caller (e.g. desktop app).
                // Silently skip — the global dispatcher is already set.
            }
        }
    });
}
