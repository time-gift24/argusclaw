//! Cookie types: Cookie, CookieKey, CookieEvent

use chrono::{DateTime, Utc};

/// Single Cookie entry.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
    pub expires: Option<DateTime<Utc>>,
}

impl Cookie {
    /// Create a cookie with minimal fields (for testing).
    #[cfg(test)]
    pub fn new(name: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: String::new(),
            domain: domain.into(),
            path: "/".to_string(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        }
    }
}

/// Unique identifier for a cookie (name + domain + path).
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct CookieKey {
    pub name: String,
    pub domain: String,
    pub path: String,
}

impl CookieKey {
    /// Create a key from a cookie reference.
    pub fn from_cookie(cookie: &Cookie) -> Self {
        Self {
            name: cookie.name.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
        }
    }
}

/// Cookie change event for broadcast subscribers.
#[derive(Clone, Debug)]
pub enum CookieEvent {
    Added(Cookie),
    Updated(Cookie),
    Removed { domain: String, name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_key_from_cookie() {
        let cookie = Cookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: "example.com".into(),
            path: "/app".into(),
            secure: true,
            http_only: false,
            same_site: Some("Lax".into()),
            expires: None,
        };

        let key = CookieKey::from_cookie(&cookie);

        assert_eq!(key.name, "session");
        assert_eq!(key.domain, "example.com");
        assert_eq!(key.path, "/app");
    }

    #[test]
    fn cookie_key_equality() {
        let k1 = CookieKey {
            name: "a".into(),
            domain: "ex.com".into(),
            path: "/".into(),
        };
        let k2 = CookieKey {
            name: "a".into(),
            domain: "ex.com".into(),
            path: "/".into(),
        };
        let k3 = CookieKey {
            name: "b".into(),
            domain: "ex.com".into(),
            path: "/".into(),
        };

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }
}
