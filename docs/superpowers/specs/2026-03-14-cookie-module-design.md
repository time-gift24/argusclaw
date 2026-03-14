# Cookie 模块设计文档

## 概述

为 ArgusClaw 添加 Cookie 管理能力，通过 Chrome DevTools Protocol (CDP) 实时监听 Chrome 浏览器的 Cookie 变化，提供内存存储和工具接口。

## 目标

- 实时获取 Chrome 中的 Cookie
- 内存存储，按域名索引
- 提供 NamedTool 接口供 LLM 调用
- 支持订阅 Cookie 变化事件

## 非目标

- 持久化存储
- 支持非 Chromium 浏览器
- 修改/注入 Cookie（后续可扩展）

## 架构

```
┌──────────────┐     CDP/WebSocket      ┌─────────────────────┐
│   Chrome     │◄──────────────────────►│  claw::cookie       │
│  --remote    │    ws://127.0.0.1:9222 │                     │
└──────────────┘                        │  ┌───────────────┐  │
                                        │  │CookieManager  │  │
                                        │  └───────┬───────┘  │
                                        │          │          │
                                        │  ┌───────┴───────┐  │
                                        │  │               │  │
                                        │  ▼               ▼  │
                                        │ Chrome      Cookie   │
                                        │ Connection  Store    │
                                        └─────────────────────┘
```

## 模块结构

```
crates/claw/src/
├── cookie/
│   ├── mod.rs           # 模块入口，导出公共 API
│   ├── error.rs         # CookieError 类型
│   ├── manager.rs       # CookieManager 核心
│   ├── chrome.rs        # ChromeConnection (CDP 封装)
│   ├── store.rs         # CookieStore (内存存储)
│   └── types.rs         # Cookie, CookieKey, CookieEvent
```

## 核心组件

### Cookie

```rust
/// 单个 Cookie
#[derive(Clone, Debug)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
    pub expires: Option<DateTime<Utc>>,
}
```

### CookieKey

```rust
/// Cookie 唯一标识，用于 HashMap 索引
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct CookieKey {
    name: String,
    domain: String,
    path: String,
}

impl CookieKey {
    /// 从 Cookie 创建 Key
    pub fn from_cookie(cookie: &Cookie) -> Self {
        Self {
            name: cookie.name.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
        }
    }
}
```

### CookieEvent

```rust
/// Cookie 变化事件
#[derive(Clone, Debug)]
pub enum CookieEvent {
    Added(Cookie),
    Updated(Cookie),
    Removed { domain: String, name: String },
}
```

### CookieStore

```rust
/// 内存存储，按域名索引
pub struct CookieStore {
    /// domain -> Vec<Cookie>
    by_domain: HashMap<String, Vec<Cookie>>,
    /// CookieKey -> Cookie (快速查找)
    index: HashMap<CookieKey, Cookie>,
}

impl CookieStore {
    /// 插入或更新 Cookie，返回是否为更新操作
    pub fn insert(&mut self, cookie: Cookie) -> bool;

    /// 删除 Cookie
    pub fn remove(&mut self, key: &CookieKey) -> Option<Cookie>;

    /// 获取指定域名的所有 Cookie
    pub fn get_by_domain(&self, domain: &str) -> Vec<Cookie>;

    /// 获取所有 Cookie
    pub fn get_all(&self) -> Vec<Cookie>;
}
```

### ChromeConnection

```rust
/// CDP 连接封装
pub struct ChromeConnection {
    client: chromiumoxide::Browser,
}

impl ChromeConnection {
    /// 连接到 Chrome 调试端口
    pub async fn connect(port: u16) -> Result<Self, CookieError>;

    /// 启用 Network domain
    pub async fn enable_network(&self) -> Result<(), CookieError>;

    /// 获取下一个 CDP 事件
    pub async fn next_event(&self) -> Option<CdpEvent>;
}
```

### CookieManager

```rust
/// Cookie 管理器主入口
pub struct CookieManager {
    chrome: Arc<ChromeConnection>,
    store: Arc<RwLock<CookieStore>>,
    event_tx: broadcast::Sender<CookieEvent>,
    /// 用于优雅关闭监听任务
    shutdown: CancellationToken,
}

impl CookieManager {
    /// 连接到 Chrome
    pub async fn connect(port: u16) -> Result<Self, CookieError>;

    /// 获取指定域名的 Cookie
    pub async fn get_cookies(&self, domain: &str) -> Vec<Cookie>;

    /// 获取所有 Cookie
    pub async fn get_all_cookies(&self) -> Vec<Cookie>;

    /// 订阅 Cookie 变化事件
    pub fn subscribe(&self) -> broadcast::Receiver<CookieEvent>;

    /// 关闭连接，停止监听任务
    pub async fn shutdown(&self);
}
```

## AppContext 集成

```rust
// crates/claw/src/claw.rs
pub struct AppContext {
    // ... existing fields ...

    /// Cookie 管理器（可选，需启用 cookie feature）
    #[cfg(feature = "cookie")]
    cookie_manager: Option<Arc<CookieManager>>,
}

#[cfg(feature = "cookie")]
impl AppContext {
    /// 初始化 Cookie 管理（需 Chrome 已启动调试端口）
    pub async fn init_cookie_manager(&mut self, port: u16) -> Result<(), CookieError> {
        let manager = CookieManager::connect(port).await?;
        self.cookie_manager = Some(Arc::new(manager));
        Ok(())
    }

    /// 获取 Cookie 管理器
    pub fn cookie_manager(&self) -> Option<&Arc<CookieManager>> {
        self.cookie_manager.as_ref()
    }
}
```

## 事件监听流程

```
1. CookieManager::connect()
   └─> ChromeConnection::connect()
   └─> 初始化 CancellationToken
   └─> 启动 start_listener() 后台任务

2. start_listener()
   └─> chrome.enable_network()
   └─> 循环监听 CDP 事件（可被 shutdown 取消）
       ├─> Network.responseReceived
       │   └─> 解析 Set-Cookie header
       │   └─> store.insert()
       │   └─> event_tx.send(CookieEvent::Added)
       │
       └─> Network.requestWillBeSent
           └─> 解析 Cookie header
           └─> store.insert()
           └─> event_tx.send(CookieEvent::Added)

3. shutdown() 调用时
   └─> cancellation_token.cancel()
   └─> 监听任务退出
```

## 错误处理

```rust
#[derive(Debug, thiserror::Error)]
pub enum CookieError {
    #[error("Failed to connect to Chrome: {reason}")]
    ConnectionFailed { reason: String },

    #[error("Chrome not running with remote debugging port")]
    DebuggingPortNotEnabled,

    #[error("CDP error: {0}")]
    CdpError(#[from] chromiumoxide::error::CdpError),

    #[error("Invalid cookie format: {raw}")]
    InvalidCookieFormat { raw: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

## 工具集成

### GetCookiesTool

```rust
use crate::llm::ToolDefinition;
use crate::protocol::RiskLevel;
use crate::tool::{NamedTool, ToolError};

pub struct GetCookiesTool {
    cookie_manager: Arc<CookieManager>,
}

impl NamedTool for GetCookiesTool {
    fn name(&self) -> &str {
        "get_cookies"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_cookies".into(),
            description: "获取指定域名的 Cookie".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "目标域名，如 example.com"
                    }
                },
                "required": ["domain"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let domain = args["domain"].as_str().ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool_name: "get_cookies".to_string(),
                reason: "Missing required parameter: domain".to_string(),
            }
        })?;

        let cookies = self.cookie_manager.get_cookies(domain).await;

        Ok(json!({
            "cookies": cookies,
            "cookie_header": cookies.iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; ")
        }))
    }
}
```

## 依赖

```toml
# crates/claw/Cargo.toml
[dependencies]
# chromiumoxide 版本需在实现时验证兼容性（预期 0.7+）
chromiumoxide = { version = "0.7", features = ["tokio-runtime"], optional = true }
tokio-util = { version = "0.7", optional = true }  # for CancellationToken

[features]
cookie = ["chromiumoxide", "tokio-util"]
```

## 测试策略

### 单元测试

```rust
// crates/claw/src/cookie/store.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_key_from_cookie() {
        let cookie = Cookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: true,
            http_only: false,
            same_site: Some("Lax".into()),
            expires: None,
        };
        let key = CookieKey::from_cookie(&cookie);
        assert_eq!(key.name, "session");
        assert_eq!(key.domain, "example.com");
    }

    #[test]
    fn test_store_insert_and_get() {
        let mut store = CookieStore::new();
        let cookie = test_cookie("a", "example.com");
        store.insert(cookie);

        let cookies = store.get_by_domain("example.com");
        assert_eq!(cookies.len(), 1);
    }

    #[test]
    fn test_store_update() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));
        let updated = Cookie { value: "new".into(), ..test_cookie("a", "example.com") };
        let is_update = store.insert(updated);

        assert!(is_update);
        assert_eq!(store.get_by_domain("example.com")[0].value, "new");
    }
}
```

### 集成测试

```rust
// crates/claw/tests/cookie_integration_test.rs
// 需要运行 Chrome: --remote-debugging-port=9222

#[tokio::test]
#[ignore = "requires Chrome running with debugging port"]
async fn test_connect_to_chrome() {
    let manager = CookieManager::connect(9222).await;
    assert!(manager.is_ok());
}

#[tokio::test]
#[ignore = "requires Chrome running with debugging port"]
async fn test_cookie_events() {
    let manager = CookieManager::connect(9222).await.unwrap();
    let mut rx = manager.subscribe();

    // 触发 Chrome 访问网页产生 Cookie...
    // 验证事件接收
}
```

### CI 策略

- 单元测试：始终运行
- 集成测试：仅本地运行或标记 `#[ignore]`
- CI 中使用 `--skip cookie_integration` 跳过需要 Chrome 的测试

## 使用方式

### 启动 Chrome

```bash
# macOS
/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
  --remote-debugging-port=9222
```

### 代码集成

```rust
use claw::cookie::{CookieManager, CookieEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 连接到 Chrome
    let manager = CookieManager::connect(9222).await?;

    // 订阅事件
    let mut rx = manager.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            tracing::info!("Cookie event: {:?}", event);
        }
    });

    // 获取 Cookie
    let cookies = manager.get_cookies("example.com").await;
    println!("{:?}", cookies);

    // 优雅关闭
    manager.shutdown().await;

    Ok(())
}
```

## 前置条件

1. 用户需使用 `--remote-debugging-port=9222` 启动 Chrome
2. 启用 `cookie` feature: `claw = { features = ["cookie"] }`

## 后续扩展

- [ ] SetCookiesTool：注入/修改 Cookie
- [ ] DeleteCookiesTool：删除 Cookie
- [ ] 支持多 Tab 隔离
- [ ] 支持 Firefox ( Marionette Protocol )
- [ ] Chrome 断线重连机制
