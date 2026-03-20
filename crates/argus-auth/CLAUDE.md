# Argus-Auth

> 特性：账号管理、凭证存储、TokenLLMProvider

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
├── account.rs       # AccountManager、UserInfo
├── credential.rs    # CredentialStore、CredentialRecord
├── token.rs         # TokenSource、TokenLLMProvider
└── error.rs        # AuthError
```

## 核心概念

### 1. AccountManager

**AccountManager** 管理用户账号：

```rust
pub struct AccountManager {
    // 账号存储
}

pub struct UserInfo {
    pub id: String,
    pub name: String,
    pub email: String,
}
```

### 2. CredentialStore

**CredentialStore** 管理凭证：

```rust
pub struct CredentialStore {
    // 加密的凭证存储
}

pub struct CredentialRecord {
    pub provider_id: String,
    pub api_key: EncryptedSecret,
}
```

### 3. TokenLLMProvider

**TokenLLMProvider** 包装 LLM Provider 添加 token 管理：

```rust
pub struct TokenLLMProvider {
    inner: Arc<dyn LlmProvider>,
    token_source: Arc<dyn TokenSource>,
}
```

## 公共 API

```rust
use argus_auth::{AccountManager, CredentialStore, TokenLLMProvider};

// 管理账号
let account = account_manager.get_user(user_id).await?;

// 存储凭证
store.save_credential(provider_id, api_key).await?;

// 创建带 token 管理的 Provider
let provider = TokenLLMProvider::new(inner, token_source);
```

## 依赖关系

### 上游依赖
- `argus-crypto`：加密/解密
- `argus-protocol`：LlmProvider trait

### 下游消费者
- `argus-wing`：应用入口

## 设计原则

### 1. 安全存储
- 凭证加密存储
- 不明文保存 API key

### 2. Token 管理
- 统一管理 token 使用
- 支持 token 刷新
