//! argus-auth - Authentication and credential storage for ArgusWing.

pub mod account;
pub mod credential;
pub mod error;
pub mod token;

pub use account::{AccountManager, UserInfo};
pub use credential::{CredentialRecord, CredentialStore, CredentialSummary};
pub use error::AuthError;
pub use token::{SimpleTokenSource, TokenConfig, TokenContext, TokenLLMProvider, TokenSource, UserCredentialTokenSource};
