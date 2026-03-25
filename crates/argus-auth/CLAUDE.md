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
pub struct TokenLLMProvider<T> {
    inner: T,
    token_source: Arc<dyn TokenSource>,
}
```

### 4. TokenConfig 和 TokenContext

`TokenConfig` 封装 token endpoint 配置：

```rust
pub struct TokenConfig {
    pub token_url: String,       // e.g. "https://auth.example.com/oauth/token"
    pub header_name: String,    // e.g. "Authorization"
    pub header_prefix: String,   // e.g. "Bearer "
    pub refresh_interval: Duration,
}
```

`TokenContext` 持有构建 token-wrapped provider 所需的 auth 依赖：

```rust
pub struct TokenContext {
    pub account_manager: Arc<AccountManager>,   // 当前登录用户
    pub credential_store: Arc<CredentialStore>, // 存储的凭证（username/password）
    pub config: TokenConfig,                    // token endpoint 配置
}
```

通过 `ProviderManager::with_token_context()` 传入 `ProviderManager`，构建时会自动检测 `LlmProviderRecord.credential_id`，若为 `Some`，则用对应凭证包装 provider。

**流程**：
1. `CredentialStore` 在 `credentials` 表存储加密的 (username, password)
2. `LlmProviderRecord.credential_id` 引用要使用的 credential
3. `ProviderManager::build_provider_with_model()` 获取 credential，创建 `UserCredentialTokenSource`，包装 provider 为 `TokenLLMProvider`
4. `TokenLLMProvider` 在每次 LLM 请求前获取 token 并注入为 HTTP header

## 公共 API

```rust
use argus_auth::{AccountManager, CredentialStore, TokenLLMProvider, TokenContext, TokenConfig, UserCredentialTokenSource};

// 管理账号
let account = account_manager.get_user(user_id).await?;

// 存储凭证
store.add(name, username, password).await?;

// 创建带 token 管理的 Provider
let provider = TokenLLMProvider::new(inner, token_source, username, password, refresh_interval);

// 构建 TokenContext 用于 ProviderManager
let token_context = TokenContext {
    account_manager,
    credential_store,
    config: TokenConfig::new(token_url, header_name, header_prefix),
};
```

## 依赖关系

### 上游依赖
- `argus-crypto`：加密/解密
- `argus-protocol`：LlmProvider trait

### 下游消费者
- `argus-wing`：应用入口
- `argus-llm`：TokenContext for ProviderManager

## 设计原则

### 1. 安全存储
- 凭证加密存储
- 不明文保存 API key

### 2. Token 管理
- 统一管理 token 使用
- 支持 token 刷新
