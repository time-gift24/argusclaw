//! Cookie store with domain-based indexing.

use std::collections::HashMap;

use crate::cookie::types::{Cookie, CookieKey};

/// In-memory cookie storage with dual indexing.
///
/// Provides O(1) lookup by cookie key and O(1) access to all cookies
/// for a given domain.
#[derive(Debug)]
pub struct CookieStore {
    /// Fast lookup by unique cookie key (name + domain + path).
    index: HashMap<CookieKey, Cookie>,
    /// Domain-indexed cookies for efficient domain queries.
    by_domain: HashMap<String, Vec<Cookie>>,
}

impl CookieStore {
    /// Create a new empty cookie store.
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            by_domain: HashMap::new(),
        }
    }

    /// Insert or update a cookie.
    ///
    /// Returns `true` if this was an update to an existing cookie,
    /// `false` if this was a new insertion.
    pub fn insert(&mut self, cookie: Cookie) -> bool {
        let key = CookieKey::from_cookie(&cookie);
        let domain = cookie.domain.clone();
        let is_update = self.index.contains_key(&key);

        // Update the main index
        self.index.insert(key.clone(), cookie.clone());

        if is_update {
            // Update in domain list
            if let Some(domain_cookies) = self.by_domain.get_mut(&domain) {
                for c in domain_cookies.iter_mut() {
                    if c.name == cookie.name && c.path == cookie.path {
                        *c = cookie;
                        return true;
                    }
                }
            }
            // Should not reach here if index and by_domain are in sync
            true
        } else {
            // Add to domain list
            self.by_domain.entry(domain).or_default().push(cookie);
            false
        }
    }

    /// Remove a cookie by its key.
    ///
    /// Returns the removed cookie if it existed.
    pub fn remove(&mut self, key: &CookieKey) -> Option<Cookie> {
        let cookie = self.index.remove(key)?;

        // Remove from domain list
        if let Some(domain_cookies) = self.by_domain.get_mut(&cookie.domain) {
            domain_cookies
                .retain(|c| !(c.name == key.name && c.path == key.path && c.domain == key.domain));
            // Clean up empty domain entries
            if domain_cookies.is_empty() {
                self.by_domain.remove(&cookie.domain);
            }
        }

        Some(cookie)
    }

    /// Get all cookies for a specific domain.
    ///
    /// Returns an empty vector if no cookies exist for the domain.
    pub fn get_by_domain(&self, domain: &str) -> Vec<Cookie> {
        self.by_domain.get(domain).cloned().unwrap_or_default()
    }

    /// Get all cookies in the store.
    pub fn get_all(&self) -> Vec<Cookie> {
        self.index.values().cloned().collect()
    }

    /// Return the number of cookies in the store.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

impl Default for CookieStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cookie(name: &str, domain: &str) -> Cookie {
        Cookie {
            name: name.to_string(),
            value: "value".to_string(),
            domain: domain.to_string(),
            path: "/".to_string(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        }
    }

    #[test]
    fn new_store_is_empty() {
        let store = CookieStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn insert_and_get_by_domain() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));
        store.insert(test_cookie("b", "example.com"));
        store.insert(test_cookie("c", "other.com"));

        assert_eq!(store.get_by_domain("example.com").len(), 2);
        assert_eq!(store.get_by_domain("other.com").len(), 1);
        assert!(store.get_by_domain("nonexistent.com").is_empty());
    }

    #[test]
    fn insert_returns_false_for_new() {
        let mut store = CookieStore::new();
        assert!(!store.insert(test_cookie("a", "example.com")));
    }

    #[test]
    fn insert_returns_true_for_update() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));

        let updated = Cookie {
            value: "new_value".to_string(),
            ..test_cookie("a", "example.com")
        };
        assert!(store.insert(updated));
        assert_eq!(store.get_by_domain("example.com")[0].value, "new_value");
    }

    #[test]
    fn remove_existing_cookie() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));

        let key = CookieKey::from_cookie(&test_cookie("a", "example.com"));
        assert!(store.remove(&key).is_some());
        assert!(store.is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut store = CookieStore::new();
        let key = CookieKey::from_cookie(&test_cookie("a", "example.com"));
        assert!(store.remove(&key).is_none());
    }

    #[test]
    fn get_all_returns_all() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));
        store.insert(test_cookie("b", "example.com"));
        store.insert(test_cookie("c", "other.com"));

        assert_eq!(store.get_all().len(), 3);
    }

    #[test]
    fn default_creates_empty_store() {
        let store = CookieStore::default();
        assert!(store.is_empty());
    }

    #[test]
    fn remove_cleans_up_empty_domain() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));

        let key = CookieKey::from_cookie(&test_cookie("a", "example.com"));
        store.remove(&key);

        // Domain entry should be removed when last cookie is removed
        assert!(store.get_by_domain("example.com").is_empty());
    }

    #[test]
    fn update_preserves_domain_count() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));
        store.insert(test_cookie("a", "example.com")); // Update

        // Should still have exactly 1 cookie
        assert_eq!(store.len(), 1);
        assert_eq!(store.get_by_domain("example.com").len(), 1);
    }

    #[test]
    fn cookies_with_different_paths_are_distinct() {
        let mut store = CookieStore::new();

        let cookie1 = Cookie {
            path: "/".to_string(),
            ..test_cookie("session", "example.com")
        };
        let cookie2 = Cookie {
            path: "/app".to_string(),
            ..test_cookie("session", "example.com")
        };

        assert!(!store.insert(cookie1)); // New
        assert!(!store.insert(cookie2)); // New (different path)

        assert_eq!(store.len(), 2);
    }
}
