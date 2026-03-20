//! Shared HTTP client with connection pooling.

use once_cell::sync::Lazy;
use reqwest::Client;

/// Shared HTTP client with connection pooling.
/// All crates that need HTTP should use this singleton to share the connection pool.
pub static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(20)
        .use_rustls_tls()
        .build()
        .expect("failed to build HTTP client")
});
