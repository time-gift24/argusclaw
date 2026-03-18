# Argus-LLM Crate 开发指南

> 特性：LLM provider 抽象层，提供 OpenAI 兼容实现、Retry 装饰器和密钥加密管理。


- **OpenAI-compatible provider** 实现
- **Retry 机制**：指数退避重试装饰器
- **Provider 管理器**：provider 查找、实例化和测试
- **密钥管理**：主机绑定的加密存储
- **测试工具**：重试行为测试辅助

## 模块结构

```
src/
├── lib.rs                    # 公共 API 导出
├── manager.rs                # ProviderManager：provider 生命周期管理
├── retry.rs                  # RetryProvider：重试装饰器
├── secret.rs                 # ApiKeyCipher：API 密钥加密
├── test_utils.rs             # TestRetryProvider：测试工具
├── providers/
│   ├── mod.rs               # Provider 模块导出
│   └── openai_compatible.rs # OpenAI-compatible provider 实现
└── bin/
    └── cli.rs               # CLI 测试工具

examples/
└── test_real_retry.rs       # 重试行为演示

scripts/
├── test-retry-behavior.sh   # 自动化重试测试
├── demo-retry-events.sh     # 重试事件演示
└── test-real-retry.sh       # 真实 provider 测试
```

## 核心概念

### 1. Provider 管理

**ProviderManager** 是 provider 查找和实例化的高层接口：

```rust
let manager = ProviderManager::new(repository);

// 获取 provider（自动包装 retry）
let provider = manager.get_provider("provider-id")?;
let provider = manager.get_provider_with_model("provider-id", "gpt-4o")?;

// 测试连接
let result = manager.test_provider_connection("provider-id", "gpt-4o")?;
```

**关键特性**：
- 所有 provider 自动包装 `RetryProvider`
- 连接测试返回详细状态报告
- 模型验证和错误映射

### 2. Retry 装饰器模式

**RetryProvider** 实现**装饰器模式**：

```rust
let provider = OpenAiCompatibleProvider::new(config)?;
let retry_provider = RetryProvider::new(
    Arc::new(provider),
    RetryConfig { max_retries: 3 }
);
```

**重试规则**：
- ✅ **会重试**：`RequestFailed`、`RateLimited`、`InvalidResponse`、`SessionRenewalFailed`
- ❌ **不重试**：`AuthFailed`、`ModelNotAvailable`、`ContextLengthExceeded`、`SessionExpired`

**退避算法**：
```
attempt 0: 300ms ± 75ms
attempt 1: 600ms ± 150ms
attempt 2: 1200ms ± 300ms
attempt 3: 2400ms ± 600ms
attempt 4+: 5000ms ± 1250ms (capped)
```

**重试事件**：
```rust
pub enum LlmStreamEvent {
    RetryAttempt {
        attempt: u32,      // 当前尝试次数（1-indexed）
        max_retries: u32,  // 最大重试次数
        error: String,     // 触发重试的错误信息
    },
    // ...
}
```

**重要**：重试事件在流中**先于实际数据发送**（即使最终失败）。

### 3. 密钥加密

**ApiKeyCipher** 提供 API 密钥的安全存储：

```rust
let cipher = ApiKeyCipher::new(KeyMaterialSource::HostMacAddress);

// 加密
let encrypted = cipher.encrypt("sk-...")?;

// 解密
let api_key = cipher.decrypt(&encrypted.nonce, &encrypted.ciphertext)?;
```

**密钥源**：
- `HostMacAddress`：使用主机 MAC 地址（默认）
- `File`：主密钥文件 `~/.arguswing/master.key`
- `Static`：静态字节（仅测试）

**加密算法**：
- AES-256-GCM
- HKDF-SHA256 密钥派生
- 12 字节随机 nonce

### 4. OpenAI-Compatible Provider

**OpenAiCompatibleProvider** 实现完整的 `LlmProvider` trait：

**能力**：
- ✅ 推理模式（GLM-5, GLM-4.7 等模型）
- ✅ 工具调用（function calling）
- ✅ 流式响应（SSE）
- ✅ 自定义 HTTP 头

**请求映射**：
```
CompletionRequest → OpenAI chat completions format
  ├─ messages → messages
  ├─ tools → tools
  ├─ ThinkingConfig → 推理参数
  └─ multi-part content → content array
```

**错误分类**：
```
401/403 → AuthFailed
404 + "model" → ModelNotAvailable
429 → RateLimited (with retry-after)
其他 → RequestFailed
```

## 公共 API

### Provider 创建

```rust
use argus_llm::{
    create_openai_compatible_provider,
    OpenAiCompatibleFactoryConfig,
    RetryProvider, RetryConfig,
};

// 创建 provider
let config = OpenAiCompatibleFactoryConfig {
    base_url: "https://api.openai.com/v1".to_string(),
    api_key: "sk-...".to_string(),
    model: "gpt-4o-mini".to_string(),
    headers: None,
    retry_config: Some(RetryConfig { max_retries: 3 }),
};

let provider = create_openai_compatible_provider(config)?;
```

### ProviderManager

```rust
use argus_llm::ProviderManager;

let manager = ProviderManager::new(repository);

// 获取 provider
let provider = manager.get_provider("id")?;

// 测试连接
let result = manager.test_provider_connection("id", "model")?;
match result.status {
    ProviderTestStatus::Ok => println!("Connected!"),
    ProviderTestStatus::Failed => eprintln!("Failed: {}", result.error),
}
```

### 密钥管理

```rust
use argus_llm::ApiKeyCipher;
use argus_protocol::llm::KeyMaterialSource;

let cipher = ApiKeyCipher::new(KeyMaterialSource::HostMacAddress);

// 加密存储
let encrypted = cipher.encrypt("sk-...")?;
store_to_db(&encrypted.nonce, &encrypted.ciphertext);

// 解密使用
let api_key = cipher.decrypt(&nonce, &ciphertext)?;
```

### 测试工具

```rust
use argus_llm::create_test_retry_provider;

// 注入失败进行重试测试
let provider = create_test_retry_provider(
    real_provider,
    3,  // max_retries
)?;

// 前几次调用会失败，触发重试
let response = provider.complete(request).await?;
```

## CLI 工具

**argus-llm-cli** 提供测试 interface：

```bash
# 测试连接
argus-llm-cli test --provider-id <id>

# 完成提示（非流式）
argus-llm-cli complete "Hello, world!"

# 完成提示（流式）
argus-llm-cli complete --stream "Hello, world!"

# 测试重试（注入失败）
argus-llm-cli retry-test --test-retry

# Mock provider 测试
argus-llm-cli mock-test
```

**配置文件** (`llm.toml`)：
```toml
base_url = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"
```

**环境变量**：
- `ARGUS_LLM_BASE_URL`
- `ARGUS_LLM_API_KEY`
- `ARGUS_LLM_MODEL`
- `ARGUSCLAW_MASTER_KEY_PATH`

## 测试

### 单元测试

```bash
# 重试机制测试
cargo test retry_provider_emits_retry_events

# 密钥加密测试
cargo test encryption_roundtrip

# OpenAI provider 测试
cargo test openai_provider_request_serialization
```

### 集成测试

```bash
# 自动化重试行为测试
./scripts/test-retry-behavior.sh

# 重试事件演示
./scripts/demo-retry-events.sh

# 真实 provider 测试
./scripts/test-real-retry.sh
```

### 示例程序

```bash
# 真实 provider 重试测试
cargo run --example test_real_retry
```

## 开发指南

### 添加新的 Provider

1. 实现 `LlmProvider` trait（来自 `argus_protocol`）
2. 在 `src/providers/mod.rs` 中导出
3. 在 `manager.rs` 中添加工厂函数
4. 添加单元测试和集成测试

**示例**：
```rust
#[async_trait]
impl LlmProvider for MyProvider {
    fn model_name(&self) -> &str {
        "my-model"
    }

    async fn complete(&self, request: CompletionRequest)
        -> Result<CompletionResponse, LlmError>
    {
        // 实现非流式完成
    }

    async fn stream_complete(&self, request: CompletionRequest)
        -> Result<LlmEventStream, LlmError>
    {
        // 实现流式完成
    }

    // ... 其他方法
}
```

### 测试 Retry 行为

使用 `TestRetryProvider` 或 `FlakyProvider`：

```rust
use argus_llm::test_utils::TestRetryProvider;
use argus_llm::retry::{RetryProvider, RetryConfig};

let inner = Arc::new(MyProvider::new());
let test_provider = TestRetryProvider::new(
    inner,
    3,      // 前 3 次调用失败
    true,   // fail_first=true: calls 1-3 fail, call 4+ succeeds
);

let provider = RetryProvider::new(
    Arc::new(test_provider),
    RetryConfig { max_retries: 5 }
);

// 第一次调用失败 3 次后成功
let response = provider.complete(request).await?;
```

### 调试重试事件

设置日志级别：
```bash
RUST_LOG=arguswing=debug cargo run
```

查看重试日志：
```
WARN Retrying after transient error
  provider=gpt-4o-mini
  attempt=1
  max_retries=3
  delay_ms=375
  error=rate limited
```

## 设计原则

### 1. 装饰器模式
- `RetryProvider` 包装任何 `LlmProvider`
- 透明添加重试逻辑
- 不修改底层 provider

### 2. 工厂模式
- `create_openai_compatible_provider()` 统一创建接口
- 可选的 retry 包装
- 配置驱动的实例化

### 3. 策略模式
- `KeyMaterialSource` trait 支持多种密钥源
- 可插拔的密钥材料策略

### 4. 仓储模式
- `LlmProviderRepository` 用于持久化
- `ProviderManager` 使用仓储访问数据

## 依赖关系

### 上游依赖
- `argus-protocol`：核心类型（`LlmProvider`、`LlmError`、`LlmStreamEvent`）
- `argus-test-support`：Mock providers

### 下游消费者
- `argus-repository`：Provider repository 实现
- `argus-turn`：Turn 执行
- `claw`：核心应用逻辑

## 关键文件路径

| 功能 | 文件 |
|------|------|
| Provider 管理 | `src/manager.rs` |
| 重试逻辑 | `src/retry.rs` |
| 密钥加密 | `src/secret.rs` |
| OpenAI provider | `src/providers/openai_compatible.rs` |
| 测试工具 | `src/test_utils.rs` |
| CLI 工具 | `src/bin/cli.rs` |
| 重试测试 | `src/retry.rs:292-533` |
| 集成测试 | `scripts/test-retry-behavior.sh` |

## 常见问题

### Q: 为什么重试事件在流的开头？
**A**: 重试循环先于流返回执行完成，收集所有重试事件后，才创建 `RetryEventStream`。这样用户可以先看到发生了多少次重试，再看实际内容。

### Q: 如何禁用重试？
**A**: 设置 `max_retries = 0` 或直接使用底层 provider 而不包装 `RetryProvider`。

### Q: 密钥加密的性能开销？
**A**: 加密/解密仅在 provider 创建时执行一次，运行时无性能影响。

### Q: 如何支持自定义重试策略？
**A**: 当前 `RetryProvider` 使用固定的指数退避。如需自定义，可以实现自己的装饰器。

## 参考资料

- **Retry 机制来源**：[nearai/ironclaw](https://github.com/nearai/ironclaw) (MIT/Apache-2.0)
- **OpenAI API 文档**：https://platform.openai.com/docs/api-reference
- **SSE 规范**：https://html.spec.whatwg.org/multipage/server-sent-events.html
