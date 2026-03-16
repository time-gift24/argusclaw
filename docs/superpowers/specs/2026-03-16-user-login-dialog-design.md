# User Login Dialog Design

**Date:** 2026-03-16
**Status:** Draft
**Scope:** Desktop application authentication via avatar click

## Overview

Add a local authentication system to the desktop application. Users click the avatar in the top-right corner to access a login dialog. On first use, users set up credentials; subsequent visits require login. Passwords are hashed using Argon2 with MAC-address-derived salt.

## Requirements

- Username + password authentication
- Local storage only (no remote API)
- Password hashed (one-way, not encrypted)
- Minimal user data (username only)
- Session persists across app restarts
- No registration/delete logic (single-user, first-time setup only)
- Avatar appearance changes based on login state
- Logout option in profile dropdown

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend (React)                      │
├─────────────────────────────────────────────────────────────┤
│  ProfileDropdown ──► LoginDialog ──► useAuthStore (zustand) │
│         │                                                    │
│         └── Avatar (changes appearance by isLoggedIn state) │
└──────────────────────────┬──────────────────────────────────┘
                           │ Tauri invoke()
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                    Backend (Rust/Tauri)                      │
├─────────────────────────────────────────────────────────────┤
│  commands.rs                                                 │
│    - get_current_user() → Option<UserInfo>                  │
│    - setup_account(username, password) → Result<()>         │
│    - login(username, password) → Result<UserInfo>           │
│    - logout() → Result<()>                                  │
│                                                              │
│  user.rs (new module in claw)                               │
│    - UserService with Argon2 password hashing               │
│    - Uses mac_address for salt derivation (existing pattern)│
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                     Database (SQLite)                        │
├─────────────────────────────────────────────────────────────┤
│  users table                                                 │
│    - id: INTEGER PRIMARY KEY                                │
│    - username: TEXT UNIQUE NOT NULL                         │
│    - password_hash: TEXT NOT NULL                           │
│    - is_logged_in: BOOLEAN DEFAULT 0                        │
│    - created_at: TEXT                                       │
│    - updated_at: TEXT                                       │
└─────────────────────────────────────────────────────────────┘
```

## Backend Design

### Database Migration

File: `crates/claw/migrations/YYYYMMDDHHMMSS_create_users_table.sql`

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    is_logged_in BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### New Module: `crates/claw/src/user.rs`

```rust
pub struct UserInfo {
    pub username: String,
}

pub struct UserService {
    pool: SqlitePool,
}

impl UserService {
    pub async fn get_current_user(&self) -> Result<Option<UserInfo>>;
    pub async fn setup_account(&self, username: &str, password: &str) -> Result<()>;
    pub async fn login(&self, username: &str, password: &str) -> Result<UserInfo>;
    pub async fn logout(&self) -> Result<()>;
}
```

### Password Hashing Strategy

- Use `argon2` crate for password hashing
- Salt derived from MAC address via HKDF-SHA256 (follows existing `secret.rs` pattern)
- Host-bound hashing ensures passwords are tied to the machine

### Tauri Commands

Add to `crates/desktop/src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub async fn get_current_user(ctx: AppContext) -> Result<Option<UserInfo>, String>;

#[tauri::command]
pub async fn setup_account(ctx: AppContext, username: String, password: String) -> Result<(), String>;

#[tauri::command]
pub async fn login(ctx: AppContext, username: String, password: String) -> Result<UserInfo, String>;

#[tauri::command]
pub async fn logout(ctx: AppContext) -> Result<(), String>;
```

### AppContext Integration

Add `UserService` to `AppContext` in `claw.rs`.

## Frontend Design

### Zustand Auth Store

File: `crates/desktop/stores/auth-store.ts`

```typescript
interface AuthState {
  username: string | null;
  isLoggedIn: boolean;
  isLoading: boolean;

  fetchCurrentUser: () => Promise<void>;
  setupAccount: (username: string, password: string) => Promise<boolean>;
  login: (username: string, password: string) => Promise<boolean>;
  logout: () => Promise<void>;
}
```

### LoginDialog Component

File: `crates/desktop/components/auth/login-dialog.tsx`

Two modes:

**Setup Mode** (no existing user):
- Username input
- Password input
- Confirm password input
- "Create Account" button

**Login Mode** (existing user):
- Username input
- Password input
- "Login" button
- Error message display

### ProfileDropdown Changes

File: `crates/desktop/components/shadcn-studio/blocks/navbar-component-06/navbar-component-06.tsx`

Avatar appearance:
- Logged in: Primary color background, shows first letter of username
- Not logged in: Muted background, shows "?"

Dropdown content:
- Logged in: Shows username + separator + "Logout" button
- Not logged in: Shows "Login" menu item

## Data Flow

### App Startup

1. App loads
2. `useAuthStore.fetchCurrentUser()` calls `invoke('get_current_user')`
3. If user exists and `is_logged_in=1`: set `isLoggedIn=true`, store `username`
4. Otherwise: set `isLoggedIn=false`
5. Avatar displays based on state

### First-Time Setup

1. User clicks avatar (not logged in)
2. LoginDialog opens, calls `get_current_user`
3. No user exists → Dialog shows "Setup" mode
4. User enters username + password + confirm
5. `setupAccount()` calls `invoke('setup_account')`
6. Backend: hash password, store in DB, set `is_logged_in=1`
7. Frontend: close dialog, set `isLoggedIn=true`

### Login (Existing User)

1. User clicks avatar (not logged in)
2. LoginDialog opens, calls `get_current_user`
3. User exists but `is_logged_in=0` → Dialog shows "Login" mode
4. User enters username + password
5. `login()` calls `invoke('login')`
6. Backend: verify password hash, set `is_logged_in=1`
7. On success: close dialog, set `isLoggedIn=true`
8. On failure: show error in dialog

### Logout

1. User clicks avatar (logged in)
2. Dropdown shows username + "Logout"
3. User clicks "Logout"
4. `logout()` calls `invoke('logout')`
5. Backend: set `is_logged_in=0`
6. Frontend: set `isLoggedIn=false`, `username=null`

## Error Handling

### Backend Errors

```rust
pub enum UserError {
    UserAlreadyExists { username: String },
    UserNotFound { username: String },
    InvalidPassword,
    NoUserSetup,
    DatabaseError { reason: String },
    HashError { reason: String },
}
```

### Frontend Error Messages

| Scenario | Message |
|----------|---------|
| Wrong password | "Incorrect password. Please try again." |
| User not found | "User not found." |
| Username empty | "Username is required" |
| Password empty | "Password is required" |
| Passwords don't match | "Passwords do not match" |
| User already exists | "Account already set up" |
| Generic error | "An error occurred. Please try again." |

### Validation Rules

**Username:**
- Required, non-empty
- Trimmed whitespace
- Max 50 characters

**Password:**
- Required, non-empty
- Min 4 characters
- Max 100 characters

### Edge Cases

| Case | Behavior |
|------|----------|
| DB has user but app restarts | `get_current_user` returns user, `is_logged_in` persists |
| Multiple rows in users table | App logic enforces single-user, use `LIMIT 1` |
| MAC address changes | User must re-setup account (salt derivation changes) |

## File Changes Summary

### New Files

- `crates/claw/migrations/YYYYMMDDHHMMSS_create_users_table.sql`
- `crates/claw/src/user.rs`
- `crates/desktop/stores/auth-store.ts`
- `crates/desktop/components/auth/login-dialog.tsx`

### Modified Files

- `crates/claw/src/lib.rs` — Add `user` module
- `crates/claw/src/claw.rs` — Add `UserService` to `AppContext`
- `crates/claw/src/error.rs` — Add `UserError`
- `crates/desktop/src-tauri/src/commands.rs` — Add auth commands
- `crates/desktop/src-tauri/src/lib.rs` — Register new commands
- `crates/desktop/components/shadcn-studio/blocks/navbar-component-06/navbar-component-06.tsx` — Update ProfileDropdown
- `crates/desktop/app/layout.tsx` — May need updates for auth state initialization

### Dependencies

- `crates/claw/Cargo.toml` — Add `argon2` crate
