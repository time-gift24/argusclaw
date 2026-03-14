//! Cookie store.

use std::collections::HashMap;

use crate::cookie::types::{Cookie, CookieKey};

/// In-memory cookie storage.
#[allow(dead_code)]
pub struct CookieStore {
    cookies: HashMap<CookieKey, Cookie>,
}

impl CookieStore {
    /// Create a new empty cookie store.
    pub fn new() -> Self {
        Self {
            cookies: HashMap::new(),
        }
    }
}

impl Default for CookieStore {
    fn default() -> Self {
        Self::new()
    }
}
