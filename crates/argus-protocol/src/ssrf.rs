//! SSRF (Server-Side Request Forgery) protection utilities.
//!
//! Pure-sync helpers for validating URLs and IP addresses before making HTTP requests.
//! This module lives in argus-protocol so it can be used by any crate without tokio.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Url;

use crate::tool::ToolError;

/// Maximum response size in bytes (10MB).
pub const MAX_RESPONSE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum timeout in seconds.
pub const MAX_TIMEOUT_SECS: u64 = 300;

/// Checks if an IPv4 address is in a blocked range.
///
/// Blocked ranges:
/// - Loopback: 127.0.0.0/8
/// - Link-local: 169.254.0.0/16
/// - Private: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
/// - CGNAT: 100.64.0.0/10
/// - Multicast: 224.0.0.0/4, 240.0.0.0/4 (Class D and E)
/// - Cloud metadata: 169.254.169.254, 169.254.169.253, 169.254.169.249-252
#[inline]
pub fn is_blocked_ip_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();

    // Loopback (127.0.0.0/8)
    if octets[0] == 127 {
        return true;
    }

    // Link-local (169.254.0.0/16)
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }

    // Private networks
    // 10.0.0.0/8
    if octets[0] == 10 {
        return true;
    }
    // 172.16.0.0/12
    if octets[0] == 172 && (16..=31).contains(&octets[1]) {
        return true;
    }
    // 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }

    // CGNAT (100.64.0.0/10)
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return true;
    }

    // Multicast (224.0.0.0/4)
    if (octets[0] & 0xF0) == 224 {
        return true;
    }

    // Class E / reserved (240.0.0.0/4)
    if (octets[0] & 0xF0) == 240 {
        return true;
    }

    // Cloud metadata service IPs (169.254.169.254 and neighbors)
    if octets[0] == 169 && octets[1] == 254 {
        let last_two = u16::from(octets[2]) << 8 | u16::from(octets[3]);
        // 169.254.169.249 through 169.254.169.254 (AWS/GCP/Azure metadata)
        if octets[2] == 169 && (249..=254).contains(&octets[3]) {
            return true;
        }
        // 169.254.169.253 (alibaba metadata)
        if octets[2] == 169 && octets[3] == 253 {
            return true;
        }
    }

    false
}

/// Checks if an IP address (IPv4 or IPv6) is blocked.
///
/// For IPv6, additionally checks IPv4-mapped addresses (::ffff:x.x.x.x).
#[inline]
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ip_v4(v4),
        IpAddr::V6(v6) => is_blocked_ip_v6(v6),
    }
}

/// Checks if an IPv6 address is in a blocked range.
///
/// Blocked ranges:
/// - Loopback: ::1
/// - Unspecified: ::
/// - IPv4-mapped: ::ffff:x.x.x.x
/// - Link-local: fe80::/10
/// - Unique local: fc00::/7
/// - Multicast: ff00::/8
#[inline]
pub fn is_blocked_ip_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();

    // Loopback ::1
    if ip.is_loopback() {
        return true;
    }

    // Unspecified ::
    if ip.is_unspecified() {
        return true;
    }

    // IPv4-mapped (::ffff:x.x.x.x) - check the IPv4 part
    if ip.is_ipv4_mapped() {
        // Convert to IPv4 and check
        let v4 = ip.to_ipv4_mapped();
        return is_blocked_ip_v4(v4);
    }

    // Link-local (fe80::/10) - first byte 0xfe, second byte 0x80-0xbf
    if segments[0] & 0xC0 == 0xFE80 {
        return true;
    }

    // Unique local (fc00::/7) - first byte 0xfc or 0xfd
    if segments[0] & 0xFE == 0xFC {
        return true;
    }

    // Multicast (ff00::/8)
    if (segments[0] & 0xFF00) == 0xFF00 {
        return true;
    }

    false
}

/// Validates a URL for security before making a request.
///
/// Checks:
/// - Only https and http schemes allowed (http is blocked by default)
/// - No localhost /127.0.0.1
/// - Host must be a valid non-empty string
///
/// Returns `Ok` if the URL passes all checks, or a `SecurityBlocked` error with reason.
pub fn validate_url(url: &Url) -> Result<(), ToolError> {
    // Scheme check: only HTTPS allowed (HTTP is blocked for security)
    match url.scheme() {
        "https" => {}
        "http" => {
            return Err(ToolError::SecurityBlocked {
                url: url.to_string(),
                reason: "HTTP scheme is not allowed. Use HTTPS.".to_string(),
            });
        }
        scheme => {
            return Err(ToolError::SecurityBlocked {
                url: url.to_string(),
                reason: format!("Only http and https schemes are allowed, got '{scheme}'"),
            });
        }
    }

    // Host must be present
    let host = url.host_str().ok_or_else(|| ToolError::SecurityBlocked {
        url: url.to_string(),
        reason: "URL must have a host".to_string(),
    })?;

    if host.is_empty() {
        return Err(ToolError::SecurityBlocked {
            url: url.to_string(),
            reason: "URL host cannot be empty".to_string(),
        });
    }

    // Block obvious localhost variants
    let host_lower = host.to_lowercase();
    if host_lower == "localhost"
        || host_lower == "127.0.0.1"
        || host_lower == "::1"
        || host_lower == "0.0.0.0"
    {
        return Err(ToolError::SecurityBlocked {
            url: url.to_string(),
            reason: format!("Localhost address '{host}' is not allowed"),
        });
    }

    // Block IPv6 loopback/unspecified forms
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            return Err(ToolError::SecurityBlocked {
                url: url.to_string(),
                reason: format!("IP address '{host}' is in a blocked range"),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_blocked_ip_v4 tests ---

    #[test]
    fn blocked_loopback() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(127, 255, 255, 255)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(127, 0, 42, 99)));
    }

    #[test]
    fn blocked_link_local() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(169, 254, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(169, 254, 255, 255)));
    }

    #[test]
    fn blocked_private_10() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(10, 0, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(10, 255, 255, 255)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(10, 42, 99, 123)));
    }

    #[test]
    fn blocked_private_172_16() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(172, 16, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(172, 16, 42, 99)));
    }

    #[test]
    fn blocked_private_172_31() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(172, 31, 255, 255)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(172, 31, 0, 1)));
    }

    #[test]
    fn not_blocked_private_172_15() {
        // 172.16.0.0/12 only covers 172.16-31.x.x
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(172, 15, 255, 255)));
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(172, 15, 0, 0)));
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(172, 32, 0, 0)));
    }

    #[test]
    fn blocked_private_192_168() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(192, 168, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(192, 168, 255, 255)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(192, 168, 42, 99)));
    }

    #[test]
    fn not_blocked_private_192_169() {
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(192, 169, 0, 1)));
    }

    #[test]
    fn blocked_cgnat() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(100, 64, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(100, 127, 255, 255)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(100, 96, 42, 99)));
    }

    #[test]
    fn not_blocked_cgnat_edges() {
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(100, 63, 255, 255)));
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(100, 128, 0, 0)));
    }

    #[test]
    fn blocked_multicast() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(224, 0, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(239, 255, 255, 255)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(224, 42, 99, 123)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(239, 0, 0, 1)));
    }

    #[test]
    fn blocked_class_e() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(240, 0, 0, 0)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(255, 255, 255, 254)));
    }

    #[test]
    fn blocked_cloud_metadata() {
        assert!(is_blocked_ip_v4(Ipv4Addr::new(169, 254, 169, 254)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(169, 254, 169, 253)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(169, 254, 169, 249)));
        assert!(is_blocked_ip_v4(Ipv4Addr::new(169, 254, 169, 252)));
    }

    #[test]
    fn not_blocked_public() {
        // Should not block public IPs
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(1, 1, 1, 1)));
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(93, 184, 216, 34))); // example.com
        assert!(!is_blocked_ip_v4(Ipv4Addr::new(142, 250, 185, 46))); // google.com
    }

    // --- is_blocked_ip_v6 tests ---

    #[test]
    fn blocked_ipv6_loopback() {
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn blocked_ipv6_unspecified() {
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0)));
    }

    #[test]
    fn blocked_ipv6_link_local() {
        // fe80::/10
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 0)));
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0xFE9F, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF)));
    }

    #[test]
    fn not_blocked_ipv6_global() {
        // Global unicast (2000::/3)
        assert!(!is_blocked_ip_v6(Ipv6Addr::new(0x2001, 0x4860, 0, 0, 0, 0, 0, 0x4868)));
    }

    #[test]
    fn blocked_ipv6_unique_local() {
        // fc00::/7
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0xFC00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0xFD00, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn blocked_ipv6_multicast() {
        // ff00::/8
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0xFF00, 0, 0, 0, 0, 0, 0, 0)));
        assert!(is_blocked_ip_v6(Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn blocked_ipv4_mapped_loopback() {
        // ::ffff:127.0.0.1
        let mapped = Ipv6Addr::new(0, 0, 0, 0, 0, 0xFFFF, 0x7F00, 0x0001);
        assert!(is_blocked_ip_v6(mapped));
    }

    #[test]
    fn blocked_ipv4_mapped_cloud_metadata() {
        // ::ffff:169.254.169.254
        let mapped = Ipv6Addr::new(0, 0, 0, 0, 0, 0xFFFF, 0xA9FE, 0xA9FE);
        assert!(is_blocked_ip_v6(mapped));
    }

    #[test]
    fn not_blocked_ipv4_mapped_public() {
        // ::ffff:8.8.8.8
        let mapped = Ipv6Addr::new(0, 0, 0, 0, 0, 0xFFFF, 0x0808, 0x0808);
        assert!(!is_blocked_ip_v6(mapped));
    }

    // --- is_blocked_ip tests ---

    #[test]
    fn blocked_ipv4_via_is_blocked_ip() {
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    }

    #[test]
    fn not_blocked_ipv4_via_is_blocked_ip() {
        assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    // --- validate_url tests ---

    #[test]
    fn validate_url_rejects_http() {
        let url = Url::parse("http://example.com").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { reason, .. } )
            if reason.contains("HTTP")));
    }

    #[test]
    fn validate_url_rejects_localhost() {
        let url = Url::parse("https://localhost/path").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { reason, .. } )
            if reason.contains("localhost")));
    }

    #[test]
    fn validate_url_rejects_127() {
        let url = Url::parse("https://127.0.0.1/path").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { .. })));
    }

    #[test]
    fn validate_url_rejects_ipv6_loopback() {
        let url = Url::parse("https://[::1]/path").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { .. })));
    }

    #[test]
    fn validate_url_rejects_0_0_0_0() {
        let url = Url::parse("https://0.0.0.0/path").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { .. })));
    }

    #[test]
    fn validate_url_rejects_private_ip() {
        let url = Url::parse("https://192.168.1.1/path").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { .. })));
    }

    #[test]
    fn validate_url_rejects_file_scheme() {
        let url = Url::parse("file:///etc/passwd").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { reason, .. } )
            if reason.contains("file")));
    }

    #[test]
    fn validate_url_rejects_ftp_scheme() {
        let url = Url::parse("ftp://example.com/file").unwrap();
        let result = validate_url(&url);
        assert!(matches!(result, Err(ToolError::SecurityBlocked { reason, .. } )
            if reason.contains("ftp")));
    }

    #[test]
    fn validate_url_accepts_https_public() {
        let url = Url::parse("https://example.com/path?query=1").unwrap();
        assert!(validate_url(&url).is_ok());
    }

    #[test]
    fn validate_url_accepts_https_with_port() {
        let url = Url::parse("https://example.com:8443/path").unwrap();
        assert!(validate_url(&url).is_ok());
    }

    #[test]
    fn validate_url_accepts_ip_public() {
        let url = Url::parse("https://1.1.1.1/path").unwrap();
        assert!(validate_url(&url).is_ok());
    }

    #[test]
    fn validate_url_accepts_ipv6_public() {
        let url = Url::parse("https://[2606:2800:220:1::248a:8273]/path").unwrap();
        assert!(validate_url(&url).is_ok());
    }
}
