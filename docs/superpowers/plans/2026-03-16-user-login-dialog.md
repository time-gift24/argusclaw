# User Login Dialog Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local authentication to the desktop app with username/password login via avatar click.

**Architecture:** Backend uses Argon2 password hashing with random salts stored in SQLite. Tauri commands expose auth API to frontend. Zustand store manages auth state. LoginDialog component handles setup/login UI modes.

**Tech Stack:** Rust (argon2, sqlx, tauri), TypeScript (React, zustand, shadcn/ui)

---

## File Structure

### New Files

| File | Purpose |
|------|---------|
| `crates/claw/migrations/<timestamp>_create_users_table.sql` | Database schema for users |
| `crates/claw/src/user/mod.rs` | User module definition |
| `crates/claw/src/user/service.rs` | UserService implementation |
| `crates/claw/src/user/error.rs` | UserError definition |
| `crates/desktop/components/auth/use-auth-store.ts` | Zustand auth store |
| `crates/desktop/components/auth/login-dialog.tsx` | Login/Setup dialog |

### Modified Files

| File | Changes |
|------|---------|
| `crates/claw/src/lib.rs` | Add user module, re-export UserInfo |
| `crates/claw/src/claw.rs` | Add UserService to AppContext |
| `crates/claw/src/error.rs` | Add UserError variant to AgentError |
| `crates/claw/Cargo.toml` | Add argon2 dependency |
| `crates/desktop/src-tauri/src/commands.rs` | Add auth Tauri commands |
| `crates/desktop/src-tauri/src/lib.rs` | Register auth commands |
| `crates/desktop/components/shadcn-studio/blocks/dropdown-profile.tsx` | Update ProfileDropdown with auth |
| `crates/desktop/app/layout.tsx` | Initialize auth state on mount |

---

## Chunk 1: Backend - Database and Error Types

### Task 1.1: Create Database Migration

**Files:**
- Create: `crates/claw/migrations/<timestamp>_create_users_table.sql`

- [ ] **Step 1: Create migration file**

Run in `crates/claw/` directory:
```bash
sqlx migrate add create_users_table
```

- [ ] **Step 2: Edit migration file with schema**

Replace content of the generated file with:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    password_salt TEXT NOT NULL,
    is_logged_in BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

- [ ] **Step 3: Commit**

```bash
git add crates/claw/migrations/
git commit -m "feat(claw): add users table migration

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Task 1.2: Add UserError Type

**Files:**
- Create: `crates/claw/src/user/mod.rs`
- Create: `crates/claw/src/user/error.rs`
- Modify: `crates/claw/src/error.rs`
- Modify: `crates/claw/src/lib.rs`
- Modify: `crates/claw/Cargo.toml`

- [ ] **Step 1: Create user module directory**

```bash
mkdir -p crates/claw/src/user
```

- [ ] **Step 2: Create user/error.rs**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UserError {
    #[error("User already exists: {username}")]
    UserAlreadyExists { username: String },

    #[error("User not found: {username}")]
    UserNotFound { username: String },

    #[error("Invalid password")]
    InvalidPassword,

    #[error("No user setup")]
    NoUserSetup,

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("Password hash error: {reason}")]
    HashError { reason: String },
}

pub type Result<T> = std::result::Result<T, UserError>;
```

- [ ] **Step 3: Create user/mod.rs**

```rust
mod error;
mod service;

pub use error::{Result, UserError};
pub use service::{UserInfo, UserService};
```

- [ ] **Step 4: Add UserError to AgentError in error.rs**

Add this variant to the AgentError enum:

```rust
#[error(transparent)]
User(#[from] crate::user::UserError),
```

- [ ] **Step 5: Add user module to lib.rs**

Add to module declarations:

```rust
pub mod user;

// Re-export for Tauri commands
pub use user::UserInfo;
```

- [ ] **Step 6: Add argon2 dependency to Cargo.toml**

Add to `[dependencies]` in `crates/claw/Cargo.toml`:

```toml
argon2 = { version = "0.5", features = ["std"] }
```

- [ ] **Step 7: Run cargo check**

```bash
cargo check -p claw
```

Expected: No errors

- [ ] **Step 8: Commit**

```bash
git add crates/claw/src/user/ crates/claw/src/lib.rs crates/claw/src/error.rs crates/claw/Cargo.toml
git commit -m "feat(claw): add UserError type and user module skeleton

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 2: Backend - UserService Implementation

### Task 2.1: Implement Complete UserService

**Files:**
- Create: `crates/claw/src/user/service.rs`

- [ ] **Step 1: Create complete service.rs with implementation and tests**

```rust
use crate::user::{Result, UserError};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
}

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

        sqlx::query!(r#"UPDATE users SET is_logged_in = 1 WHERE username = ?"#, username)
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

        sqlx::query!(
            r#"CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                password_salt TEXT NOT NULL,
                is_logged_in BOOLEAN NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#
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
        service.setup_account("testuser", "password123").await.unwrap();

        let user = service.get_current_user().await.unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().username, "testuser");
        assert!(service.has_any_user().await.unwrap());
    }

    #[tokio::test]
    async fn test_setup_account_fails_if_user_exists() {
        let service = setup_test_service().await;
        service.setup_account("testuser", "password123").await.unwrap();

        let result = service.setup_account("another", "another").await;
        assert!(matches!(result, Err(UserError::UserAlreadyExists { .. })));
    }

    #[tokio::test]
    async fn test_login_success() {
        let service = setup_test_service().await;
        service.setup_account("testuser", "password123").await.unwrap();
        service.logout().await.unwrap();

        let user = service.login("testuser", "password123").await.unwrap();
        assert_eq!(user.username, "testuser");
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        let service = setup_test_service().await;
        service.setup_account("testuser", "password123").await.unwrap();
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
        service.setup_account("testuser", "password123").await.unwrap();

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
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p claw --lib user:: -- --test-threads=1
```

Expected: All 8 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/user/
git commit -m "feat(claw): implement UserService with Argon2 password hashing

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Task 2.2: Integrate UserService with AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

- [ ] **Step 1: Add UserService to AppContext**

Add field to AppContext struct:

```rust
pub struct AppContext {
    // ... existing fields ...
    pub user: user::UserService,
}
```

- [ ] **Step 2: Initialize UserService in AppContext::init()**

Add to the init function after pool creation:

```rust
let user = user::UserService::new(pool.clone());
```

And include in the returned struct:

```rust
Self {
    // ... existing fields ...
    user,
}
```

- [ ] **Step 3: Run cargo check**

```bash
cargo check -p claw
```

Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/claw/src/claw.rs
git commit -m "feat(claw): add UserService to AppContext

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 3: Backend - Tauri Commands

### Task 3.1: Add Auth Tauri Commands

**Files:**
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add auth commands to commands.rs**

Add to the end of commands.rs:

```rust
// ========== Auth Commands ==========

#[tauri::command]
pub async fn get_current_user(ctx: AppContext) -> Result<Option<claw::UserInfo>, String> {
    ctx.user
        .get_current_user()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn has_any_user(ctx: AppContext) -> Result<bool, String> {
    ctx.user.has_any_user().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn setup_account(
    ctx: AppContext,
    username: String,
    password: String,
) -> Result<(), String> {
    ctx.user
        .setup_account(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn login(
    ctx: AppContext,
    username: String,
    password: String,
) -> Result<claw::UserInfo, String> {
    ctx.user
        .login(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn logout(ctx: AppContext) -> Result<(), String> {
    ctx.user.logout().await.map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Register commands in lib.rs**

Add to the invoke_handler in `src-tauri/src/lib.rs`:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    commands::get_current_user,
    commands::has_any_user,
    commands::setup_account,
    commands::login,
    commands::logout,
])
```

- [ ] **Step 3: Run cargo check**

```bash
cargo check -p desktop
```

Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/desktop/src-tauri/src/commands.rs crates/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add auth Tauri commands

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 4: Frontend - Auth Store and Login Dialog

### Task 4.1: Create Auth Store

**Files:**
- Create: `crates/desktop/components/auth/use-auth-store.ts`

- [ ] **Step 1: Create auth directory**

```bash
mkdir -p crates/desktop/components/auth
```

- [ ] **Step 2: Create use-auth-store.ts**

```typescript
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

interface UserInfo {
  username: string;
}

interface AuthState {
  username: string | null;
  isLoggedIn: boolean;
  isLoading: boolean;
  hasUser: boolean | null; // null = not checked yet

  fetchCurrentUser: () => Promise<void>;
  checkHasUser: () => Promise<boolean>;
  setupAccount: (username: string, password: string) => Promise<{ success: boolean; error?: string }>;
  login: (username: string, password: string) => Promise<{ success: boolean; error?: string }>;
  logout: () => Promise<void>;
}

export const useAuthStore = create<AuthState>((set, get) => ({
  username: null,
  isLoggedIn: false,
  isLoading: true,
  hasUser: null,

  fetchCurrentUser: async () => {
    try {
      const user = await invoke<UserInfo | null>('get_current_user');
      set({
        username: user?.username ?? null,
        isLoggedIn: user !== null,
        isLoading: false,
      });
    } catch {
      set({ username: null, isLoggedIn: false, isLoading: false });
    }
  },

  checkHasUser: async () => {
    try {
      const hasUser = await invoke<boolean>('has_any_user');
      set({ hasUser });
      return hasUser;
    } catch {
      set({ hasUser: false });
      return false;
    }
  },

  setupAccount: async (username: string, password: string) => {
    try {
      await invoke('setup_account', { username, password });
      set({ username, isLoggedIn: true });
      return { success: true };
    } catch (error) {
      return { success: false, error: String(error) };
    }
  },

  login: async (username: string, password: string) => {
    try {
      const user = await invoke<UserInfo>('login', { username, password });
      set({ username: user.username, isLoggedIn: true });
      return { success: true };
    } catch (error) {
      return { success: false, error: String(error) };
    }
  },

  logout: async () => {
    try {
      await invoke('logout');
      set({ username: null, isLoggedIn: false });
    } catch {
      // Ignore logout errors
    }
  },
}));
```

- [ ] **Step 3: Verify zustand is installed**

```bash
grep -q '"zustand"' crates/desktop/package.json || npm install zustand --prefix crates/desktop
```

- [ ] **Step 4: Commit**

```bash
git add crates/desktop/components/auth/use-auth-store.ts
git commit -m "feat(desktop): add auth zustand store

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Task 4.2: Create Login Dialog Component

**Files:**
- Create: `crates/desktop/components/auth/login-dialog.tsx`

- [ ] **Step 1: Create login-dialog.tsx**

```tsx
'use client';

import { useState, useEffect, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useAuthStore } from './use-auth-store';

interface LoginDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function LoginDialog({ open, onOpenChange }: LoginDialogProps) {
  const { checkHasUser, setupAccount, login } = useAuthStore();
  const [mode, setMode] = useState<'setup' | 'login'>('setup');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isCheckingMode, setIsCheckingMode] = useState(true);

  // Determine mode when dialog opens
  const checkMode = useCallback(async () => {
    if (open) {
      setIsCheckingMode(true);
      const hasUser = await checkHasUser();
      setMode(hasUser ? 'login' : 'setup');
      setIsCheckingMode(false);
    }
  }, [open, checkHasUser]);

  useEffect(() => {
    checkMode();
  }, [checkMode]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setIsLoading(true);

    try {
      if (mode === 'setup') {
        // Validation
        if (!username.trim()) {
          setError('Username is required');
          setIsLoading(false);
          return;
        }
        if (!password) {
          setError('Password is required');
          setIsLoading(false);
          return;
        }
        if (password !== confirmPassword) {
          setError('Passwords do not match');
          setIsLoading(false);
          return;
        }
        if (password.length < 4) {
          setError('Password must be at least 4 characters');
          setIsLoading(false);
          return;
        }

        const result = await setupAccount(username.trim(), password);
        if (result.success) {
          onOpenChange(false);
          resetForm();
        } else {
          setError('Account already set up');
          setMode('login');
        }
      } else {
        // Login mode
        if (!username.trim()) {
          setError('Username is required');
          setIsLoading(false);
          return;
        }
        if (!password) {
          setError('Password is required');
          setIsLoading(false);
          return;
        }

        const result = await login(username.trim(), password);
        if (result.success) {
          onOpenChange(false);
          resetForm();
        } else {
          setError('Incorrect password. Please try again.');
        }
      }
    } finally {
      setIsLoading(false);
    }
  };

  const resetForm = () => {
    setUsername('');
    setPassword('');
    setConfirmPassword('');
    setError('');
  };

  const handleOpenChange = (newOpen: boolean) => {
    if (!newOpen) {
      resetForm();
    }
    onOpenChange(newOpen);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {isCheckingMode ? 'Loading...' : mode === 'setup' ? 'Set Up Your Account' : 'Login'}
          </DialogTitle>
          <DialogDescription>
            {isCheckingMode
              ? ''
              : mode === 'setup'
                ? 'Create your account to get started'
                : 'Enter your credentials to continue'}
          </DialogDescription>
        </DialogHeader>

        {isCheckingMode ? (
          <div className="py-8 text-center text-muted-foreground">Checking...</div>
        ) : (
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="Enter username"
                disabled={isLoading}
                maxLength={50}
                autoFocus
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Enter password"
                disabled={isLoading}
                maxLength={100}
              />
            </div>

            {mode === 'setup' && (
              <div className="space-y-2">
                <Label htmlFor="confirmPassword">Confirm Password</Label>
                <Input
                  id="confirmPassword"
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder="Confirm password"
                  disabled={isLoading}
                  maxLength={100}
                />
              </div>
            )}

            {error && <p className="text-sm text-destructive">{error}</p>}

            <Button type="submit" className="w-full" disabled={isLoading}>
              {isLoading
                ? 'Please wait...'
                : mode === 'setup'
                  ? 'Create Account'
                  : 'Login'}
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/components/auth/login-dialog.tsx
git commit -m "feat(desktop): add LoginDialog component

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Task 4.3: Update ProfileDropdown

**Files:**
- Modify: `crates/desktop/components/shadcn-studio/blocks/dropdown-profile.tsx`

- [ ] **Step 1: Read current dropdown-profile.tsx**

First, read the file to understand its current structure:

```bash
cat crates/desktop/components/shadcn-studio/blocks/dropdown-profile.tsx
```

- [ ] **Step 2: Update dropdown-profile.tsx with auth integration**

Replace the entire file content with:

```tsx
'use client';

import { useState } from 'react';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useAuthStore } from '@/components/auth/use-auth-store';
import { LoginDialog } from '@/components/auth/login-dialog';
import { UserRoundIcon } from '@hugeicons/react';

export function DropdownProfile() {
  const { username, isLoggedIn, logout } = useAuthStore();
  const [loginDialogOpen, setLoginDialogOpen] = useState(false);

  const handleAvatarClick = () => {
    if (!isLoggedIn) {
      setLoginDialogOpen(true);
    }
  };

  const handleLogout = async () => {
    await logout();
  };

  const getAvatarFallback = () => {
    if (isLoggedIn && username) {
      return username.charAt(0).toUpperCase();
    }
    return '?';
  };

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            className="relative h-10 w-10 rounded-full"
            onClick={handleAvatarClick}
          >
            <Avatar className={`h-10 w-10 ${isLoggedIn ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'}`}>
              <AvatarImage src="" alt={username ?? 'User'} />
              <AvatarFallback>{getAvatarFallback()}</AvatarFallback>
            </Avatar>
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent className="w-56" align="end" forceMount>
          {isLoggedIn ? (
            <>
              <DropdownMenuLabel className="font-normal">
                <div className="flex flex-col space-y-1">
                  <p className="text-sm font-medium leading-none">{username}</p>
                </div>
              </DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={handleLogout}>
                <UserRoundIcon className="mr-2 h-4 w-4" />
                <span>Logout</span>
              </DropdownMenuItem>
            </>
          ) : (
            <DropdownMenuItem onClick={() => setLoginDialogOpen(true)}>
              <UserRoundIcon className="mr-2 h-4 w-4" />
              <span>Login</span>
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      <LoginDialog open={loginDialogOpen} onOpenChange={setLoginDialogOpen} />
    </>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/components/shadcn-studio/blocks/dropdown-profile.tsx
git commit -m "feat(desktop): update ProfileDropdown with auth state

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Task 4.4: Initialize Auth State on App Mount

**Files:**
- Modify: `crates/desktop/app/layout.tsx`

- [ ] **Step 1: Add auth initialization to layout.tsx**

Add the following import at the top of the file:

```tsx
import { useAuthStore } from '@/components/auth/use-auth-store';
```

Add the auth initialization hook inside the RootLayout component, before the return statement:

```tsx
// Initialize auth state on mount
const fetchCurrentUser = useAuthStore((state) => state.fetchCurrentUser);
// eslint-disable-next-line react-hooks/exhaustive-deps
React.useEffect(() => {
  fetchCurrentUser();
}, []);
```

If React is not already imported, add it:

```tsx
import React from 'react';
```

- [ ] **Step 2: Verify build**

```bash
cd crates/desktop && npm run build
```

Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/app/layout.tsx
git commit -m "feat(desktop): initialize auth state on app mount

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 5: Integration and Testing

### Task 5.1: Full Build Verification

- [ ] **Step 1: Run full cargo check**

```bash
cargo check --workspace
```

Expected: No errors

- [ ] **Step 2: Run claw tests**

```bash
cargo test -p claw
```

Expected: All tests pass

- [ ] **Step 3: Build desktop app**

```bash
cd crates/desktop && npm run build
```

Expected: Build succeeds

- [ ] **Step 4: Run pre-commit checks**

```bash
prek
```

Expected: All checks pass

### Task 5.2: Manual Testing

- [ ] **Step 1: Run Tauri dev to verify**

```bash
cd crates/desktop && npm run tauri dev
```

Manual verification checklist:
1. [ ] App starts without errors
2. [ ] Avatar shows "?" with muted background (not logged in)
3. [ ] Clicking avatar opens login dialog in setup mode
4. [ ] Can create account with username/password
5. [ ] Avatar shows first letter of username with primary background
6. [ ] Clicking avatar shows dropdown with username and "Logout"
7. [ ] Can logout
8. [ ] Avatar shows "?" again after logout
9. [ ] Clicking avatar shows login dialog in login mode (not setup)
10. [ ] Can login with correct credentials
11. [ ] Wrong password shows error message
12. [ ] App restart keeps login state (persistent)

### Task 5.3: Create PR

- [ ] **Step 1: Ensure all changes are committed**

```bash
git status
```

Expected: Working tree clean

- [ ] **Step 2: Push and create PR**

```bash
git push -u origin <branch-name>
gh pr create --title "feat: add user login dialog" --body "$(cat <<'EOF'
## Summary

Implements local authentication for the desktop app with username/password login via avatar click.

## Changes

### Backend (claw)
- Add `users` table migration with username, password_hash, password_salt, is_logged_in
- Add `UserService` with Argon2 password hashing
- Add `UserError` type integrated with `AgentError`

### Backend (desktop Tauri)
- Add auth Tauri commands: `get_current_user`, `has_any_user`, `setup_account`, `login`, `logout`

### Frontend
- Add zustand auth store (`use-auth-store.ts`)
- Add `LoginDialog` component with setup/login modes
- Update `ProfileDropdown` with auth state and login/logout

## Test plan

- [ ] App starts without errors
- [ ] First-time setup flow works
- [ ] Login/logout flow works
- [ ] Wrong password shows error
- [ ] Login state persists across app restarts

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Summary

| Chunk | Tasks | Estimated Commits |
|-------|-------|-------------------|
| 1. Database and Errors | 1.1, 1.2 | 2 |
| 2. UserService | 2.1, 2.2 | 2 |
| 3. Tauri Commands | 3.1 | 1 |
| 4. Frontend | 4.1, 4.2, 4.3, 4.4 | 4 |
| 5. Integration | 5.1, 5.2, 5.3 | 0-1 |

**Total: ~9-10 commits**
