//! Cookie manager.

use crate::cookie::store::CookieStore;

/// Manages cookie storage and Chrome connection.
#[allow(dead_code)]
pub struct CookieManager {
    store: CookieStore,
}

impl CookieManager {
    /// Create a new cookie manager.
    pub fn new() -> Self {
        Self {
            store: CookieStore::new(),
        }
    }
}

impl Default for CookieManager {
    fn default() -> Self {
        Self::new()
    }
}
