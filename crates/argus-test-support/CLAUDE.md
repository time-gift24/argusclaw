# Argus-Test-Support 测试辅助

> 特性：测试辅助模块，提供 Mock Provider 等测试工具。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
└── providers.rs     # Mock providers
```

## 核心概念

### 1. Mock Providers

**AlwaysFailProvider**：总是失败的 Provider（用于测试重试）：

```rust
pub struct AlwaysFailProvider;

impl LlmProvider for AlwaysFailProvider {
    // 所有调用都返回错误
}
```

**IntermittentFailureProvider**：间歇性失败的 Provider：

```rust
pub struct IntermittentFailureProvider {
    failure_count: u32,
    interval: u32,
}

impl LlmProvider for IntermittentFailureProvider {
    // 前 N 次调用失败，之后成功
}
```

## 公共 API

```rust
use argus_test_support::{AlwaysFailProvider, IntermittentFailureProvider};

// 测试重试机制
let provider = AlwaysFailProvider::new();
let retry_provider = RetryProvider::new(Arc::new(provider), RetryConfig { max_retries: 3 });
```

## 依赖关系

### 上游依赖
- `argus-protocol`：LlmProvider trait

### 下游消费者
- 各 crate 的测试代码

## 设计原则

### 1. 仅测试使用
- 此 crate 仅用于测试
- 不应在生产代码中使用
