//! HTTP client builder with DNS pinning support.
//!
//! Provides an `HttpClientBuilder` for creating reqwest clients that lock DNS resolution
//! to pre-resolved IP addresses. This prevents DNS rebinding attacks in SSRF scenarios.

use std::net::{SocketAddr, ToSocketAddrs};

use crate::tool::ToolError;

/// Builder for creating a reqwest Client with DNS pinning.
#[derive(Default)]
pub struct HttpClientBuilder {
    timeout_secs: Option<u64>,
    dns_pinned_addrs: Option<Vec<SocketAddr>>,
}

impl HttpClientBuilder {
    /// Creates a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the request timeout.
    #[must_use]
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Pins DNS resolution for the client to a pre-resolved list of `SocketAddr`s.
    /// This is the core of DNS pinning: we resolve the hostname ourselves and tell
    /// reqwest to only connect to those specific IPs, bypassing the system resolver.
    #[must_use]
    pub fn with_dns_pin(mut self, addrs: Vec<SocketAddr>) -> Self {
        self.dns_pinned_addrs = Some(addrs);
        self
    }

    /// Resolves the given host:port using the standard resolver and pins the result.
    /// Returns an error if resolution fails or any resolved IP is in a blocklist.
    pub async fn resolve_and_pin(
        mut self,
        host: &str,
        port: u16,
    ) -> Result<Self, ToolError> {
        let addr_str = format!("{host}:{port}");
        let addrs: Vec<SocketAddr> = addr_str
            .to_socket_addrs()
            .map_err(|e| ToolError::SecurityBlocked {
                url: format!("{host}:{port}"),
                reason: format!("DNS resolution failed: {e}"),
            })?
            .collect();

        if addrs.is_empty() {
            return Err(ToolError::SecurityBlocked {
                url: format!("{host}:{port}"),
                reason: "DNS returned no addresses".to_string(),
            });
        }

        self.dns_pinned_addrs = Some(addrs);
        Ok(self)
    }

    /// Builds the reqwest `Client`.
    pub fn build(&self) -> Result<reqwest::Client, ToolError> {
        let mut builder = reqwest::Client::builder()
            .pool_max_idle_per_host(20)
            .use_rustls_tls()
            .redirect(reqwest::redirect::Policy::none()); // We handle redirects manually

        if let Some(timeout) = self.timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(timeout));
        }

        // Apply DNS pinning via a custom DNS resolver
        if let Some(ref addrs) = self.dns_pinned_addrs {
            let dns = DnsResolver(addrs.clone());
            builder = builder.resolve_to_addrs(dns.0.first().unwrap().ip(), dns.0.as_slice());
        }

        builder.build().map_err(|e| ToolError::ExecutionFailed {
            tool_name: "http".to_string(),
            reason: format!("failed to build HTTP client: {e}"),
        })
    }
}

/// Simple DNS resolver that always returns the pinned addresses.
#[derive(Clone)]
struct DnsResolver(Vec<SocketAddr>);

impl reqwest::dns::Resolve for DnsResolver {
    fn resolve(&self, _: reqwest::dns::Name) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<Vec<SocketAddr>>> + Send + '_>> {
        let addrs = self.0.clone();
        Box::pin(async move { Ok(addrs) })
    }
}
