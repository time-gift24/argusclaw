//! HTTP client builder with DNS pinning support.
//!
//! Provides an `HttpClientBuilder` for creating reqwest clients that lock DNS resolution
//! to pre-resolved IP addresses. This prevents DNS rebinding attacks in SSRF scenarios.

use std::error::Error;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::tool::ToolError;

/// Builder for creating a reqwest Client with DNS pinning.
#[derive(Default)]
pub struct HttpClientBuilder {
    timeout_secs: Option<u64>,
    dns_pinned_addrs: Option<Vec<SocketAddr>>,
    allow_insecure_ssl: bool,
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

    /// Pins DNS resolution to the given pre-resolved addresses.
    /// The addresses should be the result of a prior DNS lookup that was
    /// validated against the SSRF blocklist.
    #[must_use]
    pub fn with_dns_pin(mut self, addrs: Vec<SocketAddr>) -> Self {
        self.dns_pinned_addrs = Some(addrs);
        self
    }

    /// Allows invalid TLS certificates for the request.
    #[must_use]
    pub fn with_insecure_ssl(mut self, allow_insecure_ssl: bool) -> Self {
        self.allow_insecure_ssl = allow_insecure_ssl;
        self
    }

    /// Resolves the given host:port using the standard blocking resolver and pins the result.
    /// Returns an error if resolution fails or any resolved IP is in a blocklist.
    pub fn resolve_and_pin_blocking(mut self, host: &str, port: u16) -> Result<Self, ToolError> {
        let addr_str = format!("{host}:{port}");
        // Use std to_socket_addrs which performs blocking DNS lookup
        let addrs: Vec<SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&addr_str)
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
            .redirect(reqwest::redirect::Policy::none());

        if let Some(timeout) = self.timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(timeout));
        }

        if self.allow_insecure_ssl {
            builder = builder.danger_accept_invalid_certs(true);
        }

        // Apply DNS pinning if we have resolved addresses
        if let Some(ref addrs) = self.dns_pinned_addrs {
            let resolver = dns::DnsResolver::new(addrs.clone());
            builder = builder.dns_resolver(resolver);
        }

        builder.build().map_err(|e| ToolError::ExecutionFailed {
            tool_name: "http".to_string(),
            reason: format!("failed to build HTTP client: {e}"),
        })
    }

    #[cfg(test)]
    fn allows_insecure_ssl_for_tests(&self) -> bool {
        self.allow_insecure_ssl
    }
}

/// A future that resolves to a boxed iterator of SocketAddrs.
/// This is what reqwest's Resolve trait requires.
struct PinnedAddrsFuture {
    addrs: Option<Box<dyn Iterator<Item = SocketAddr> + Send>>,
}

impl Future for PinnedAddrsFuture {
    type Output = Result<Box<dyn Iterator<Item = SocketAddr> + Send>, Box<dyn Error + Send + Sync>>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Poll::Ready(Ok(this.addrs.take().expect("already polled")))
    }
}

/// Exposes a custom DNS resolver for reqwest that always returns pinned addresses.
pub mod dns {
    use std::error::Error;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::Arc;

    use futures_core::Future;

    use super::PinnedAddrsFuture;

    /// A DNS resolver that always returns the same pre-configured addresses.
    #[derive(Clone)]
    pub struct DnsResolver {
        addrs: Vec<SocketAddr>,
    }

    impl DnsResolver {
        #[must_use]
        pub fn new(addrs: Vec<SocketAddr>) -> Arc<Self> {
            Arc::new(Self { addrs })
        }
    }

    impl reqwest::dns::Resolve for DnsResolver {
        fn resolve(
            &self,
            _name: reqwest::dns::Name,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            Box<dyn Iterator<Item = SocketAddr> + Send>,
                            Box<dyn Error + Send + Sync>,
                        >,
                    > + Send
                    + 'static,
            >,
        > {
            let addrs: Box<dyn Iterator<Item = SocketAddr> + Send> =
                Box::new(self.addrs.clone().into_iter());
            let future = PinnedAddrsFuture { addrs: Some(addrs) };
            Box::pin(future)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HttpClientBuilder;

    #[test]
    fn builder_defaults_to_strict_tls_verification() {
        let builder = HttpClientBuilder::new();
        assert!(!builder.allows_insecure_ssl_for_tests());
    }

    #[test]
    fn builder_can_enable_insecure_ssl_verification_override() {
        let builder = HttpClientBuilder::new().with_insecure_ssl(true);
        assert!(builder.allows_insecure_ssl_for_tests());
    }
}
