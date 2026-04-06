//! Argus Server - Axum HTTP server for end-user chat.
//!
//! Provides OAuth2 login, dev OAuth2 flow, and chat API.

pub mod auth;
pub mod config;
pub mod http;
pub mod routes;
pub mod state;
