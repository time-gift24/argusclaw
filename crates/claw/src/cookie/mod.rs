//! Cookie management module for Chrome browser integration.
//!
//! Provides real-time cookie monitoring via Chrome DevTools Protocol (CDP).

mod error;
mod manager;
mod store;
mod types;

#[cfg(feature = "cookie")]
mod chrome;
#[cfg(feature = "cookie")]
mod tool;

pub use error::CookieError;
pub use manager::CookieManager;
pub use store::CookieStore;
pub use types::{Cookie, CookieEvent, CookieKey};

#[cfg(feature = "cookie")]
pub use chrome::ChromeConnection;
#[cfg(feature = "cookie")]
pub use tool::GetCookiesTool;
