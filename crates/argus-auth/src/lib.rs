//! argus-auth - Authentication and credential storage for ArgusWing.

pub mod account;
pub mod error;
pub mod token;

pub use account::{AccountManager, UserInfo};
pub use argus_repository::traits::AccountRepository;
pub use error::AuthError;
pub use token::{
    AccountTokenSource, CredentialTokenSource, SimpleTokenSource, TokenConfig, TokenContext,
    TokenLLMProvider, TokenSource,
};
