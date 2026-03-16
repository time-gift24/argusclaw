use crate::user::{Result, UserError};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
}

#[derive(Clone)]
pub struct UserService {
    pool: SqlitePool,
}

impl UserService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Returns the currently logged-in user, if any
    pub async fn get_current_user(&self) -> Result<Option<UserInfo>> {
        let row = sqlx::query_as!(
            UserInfo,
            r#"SELECT username FROM users WHERE is_logged_in = 1 LIMIT 1"#
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError {
            reason: e.to_string(),
        })?;

        Ok(row)
    }

    /// Check if any user account exists (for determining setup vs login mode)
    pub async fn has_any_user(&self) -> Result<bool> {
        let row = sqlx::query!("SELECT id FROM users LIMIT 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserError::DatabaseError {
                reason: e.to_string(),
            })?;

        Ok(row.is_some())
    }

    /// Create the initial user account (fails if user already exists)
    pub async fn setup_account(&self, username: &str, password: &str) -> Result<()> {
        // Check if user already exists
        let existing = sqlx::query!("SELECT id FROM users LIMIT 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserError::DatabaseError {
                reason: e.to_string(),
            })?;

        if existing.is_some() {
            return Err(UserError::UserAlreadyExists {
                username: username.to_string(),
            });
        }

        let (hash, salt) = hash_password(password)?;

        sqlx::query!(
            r#"INSERT INTO users (username, password_hash, password_salt, is_logged_in)
               VALUES (?, ?, ?, 1)"#,
            username,
            hash,
            salt
        )
        .execute(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    /// Authenticate user and set login state
    pub async fn login(&self, username: &str, password: &str) -> Result<UserInfo> {
        let row = sqlx::query!(
            r#"SELECT username, password_hash FROM users WHERE username = ? LIMIT 1"#,
            username
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError {
            reason: e.to_string(),
        })?
        .ok_or_else(|| UserError::UserNotFound {
            username: username.to_string(),
        })?;

        if !verify_password(password, &row.password_hash)? {
            return Err(UserError::InvalidPassword);
        }

        sqlx::query!(
            r#"UPDATE users SET is_logged_in = 1 WHERE username = ?"#,
            username
        )
        .execute(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError {
            reason: e.to_string(),
        })?;

        Ok(UserInfo {
            username: row.username,
        })
    }

    /// Clear login state
    pub async fn logout(&self) -> Result<()> {
        sqlx::query!(r#"UPDATE users SET is_logged_in = 0"#)
            .execute(&self.pool)
            .await
            .map_err(|e| UserError::DatabaseError {
                reason: e.to_string(),
            })?;

        Ok(())
    }
}

// Helper functions for password hashing

fn hash_password(password: &str) -> Result<(String, String)> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| UserError::HashError {
            reason: e.to_string(),
        })?
        .to_string();

    Ok((hash, salt.to_string()))
}

fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(hash).map_err(|e| UserError::HashError {
        reason: e.to_string(),
    })?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_test_service() -> UserService {
        let pool = SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .expect("Failed to create in-memory database");

        sqlx::query(
            r#"CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                password_salt TEXT NOT NULL,
                is_logged_in BOOLEAN NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        )
        .execute(&pool)
        .await
        .expect("Failed to create users table");

        UserService::new(pool)
    }

    #[tokio::test]
    async fn test_has_any_user_returns_false_initially() {
        let service = setup_test_service().await;
        assert!(!service.has_any_user().await.unwrap());
    }

    #[tokio::test]
    async fn test_setup_account_creates_user() {
        let service = setup_test_service().await;
        service
            .setup_account("testuser", "password123")
            .await
            .unwrap();

        let user = service.get_current_user().await.unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().username, "testuser");
        assert!(service.has_any_user().await.unwrap());
    }

    #[tokio::test]
    async fn test_setup_account_fails_if_user_exists() {
        let service = setup_test_service().await;
        service
            .setup_account("testuser", "password123")
            .await
            .unwrap();

        let result = service.setup_account("another", "another").await;
        assert!(matches!(result, Err(UserError::UserAlreadyExists { .. })));
    }

    #[tokio::test]
    async fn test_login_success() {
        let service = setup_test_service().await;
        service
            .setup_account("testuser", "password123")
            .await
            .unwrap();
        service.logout().await.unwrap();

        let user = service.login("testuser", "password123").await.unwrap();
        assert_eq!(user.username, "testuser");
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        let service = setup_test_service().await;
        service
            .setup_account("testuser", "password123")
            .await
            .unwrap();
        service.logout().await.unwrap();

        let result = service.login("testuser", "wrongpassword").await;
        assert!(matches!(result, Err(UserError::InvalidPassword)));
    }

    #[tokio::test]
    async fn test_login_nonexistent_user() {
        let service = setup_test_service().await;

        let result = service.login("nonexistent", "password").await;
        assert!(matches!(result, Err(UserError::UserNotFound { .. })));
    }

    #[tokio::test]
    async fn test_logout_clears_login_state() {
        let service = setup_test_service().await;
        service
            .setup_account("testuser", "password123")
            .await
            .unwrap();

        service.logout().await.unwrap();

        let user = service.get_current_user().await.unwrap();
        assert!(user.is_none());
    }

    #[tokio::test]
    async fn test_password_hashing_works() {
        let (hash, salt) = hash_password("testpassword").unwrap();
        assert!(!hash.is_empty());
        assert!(!salt.is_empty());
        assert!(verify_password("testpassword", &hash).unwrap());
        assert!(!verify_password("wrongpassword", &hash).unwrap());
    }
}
