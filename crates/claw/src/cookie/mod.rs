//! Cookie extraction via Chrome DevTools Protocol

mod error;
mod types;

pub use error::{CookieError, CookieResult};
pub use types::Cookie;

/// Get cookies for a specific domain from Chrome CDP
pub async fn get_cookies(
    _cdp_ws_url: &str,
    _domain: &str,
) -> CookieResult<Vec<Cookie>> {
    todo!("Implement in Task 2")
}