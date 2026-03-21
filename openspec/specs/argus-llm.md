# argus-llm — LLM Provider 抽象层

## 职责

提供 LLM provider 的实现和组合层。它只依赖 `argus-protocol`，不依赖其他 argus-* crates。

```
接收 LlmProviderRecord (base_url, api_key, model, ...)
                    │
                    ▼
           ProviderManager
                    │
          ┌─────────┴─────────┐
          ▼                   ▼
  build_provider()    test_connection()
          │                   │
          ▼                   ▼
  OpenAiCompatible   ──wrap──▶ RetryProvider (装饰器)
                                    │
                                    ▼
                            OpenAiCompatibleProvider
                                    │
                                    ▼
                              HTTP → OpenAI-compatible API
```

## 核心抽象

### LlmProvider trait

由 `argus-protocol` 定义。本 crate 提供其实现。

### ProviderManager

**职责**：从持久化存储（LlmProviderRepository）查找 record，构建 provider 实例，测试连接。

关键方法：
- `get_provider(id)` — 查找 record，构建 instance，wrap RetryProvider
- `get_provider_with_model(id, model)` — 带指定 model 构建
- `test_provider_connection(id, model)` — 连接测试（发一个带 echo 工具的请求）
- `upsert_provider(record)` — 创建或更新 provider record

**注意**：所有 provider 自动被 RetryProvider 包装。调用方不需要手动 wrap。

### RetryProvider（装饰器）

**职责**：透明添加重试能力到任何 LlmProvider。

**可重试错误**：`RequestFailed`、`RateLimited`、`InvalidResponse`、`SessionRenewalFailed`

**不重试**：`AuthFailed`、`ModelNotAvailable`、`ContextLengthExceeded`、`SessionExpired`

退避算法：
```
attempt 0: ~300ms ± 75ms
attempt 1: ~600ms ± 150ms
attempt 2: ~1200ms ± 300ms
attempt 3+: ~5000ms ± 1250ms（封顶）
```

特殊：`RateLimited` 错误优先使用 server 返回的 `retry_after` 时间。

**流式处理的边界情况**：RetryProvider 在流式 API 上也工作。流式启动失败（网络错误）会重试；一旦流返回 `Ok(stream)`，流本身不再重试（数据已经在传输）。

**RetryAttempt 事件**：每次重试前，RetryProvider 向流中注入一个 `LlmStreamEvent::RetryAttempt` 事件（attempt number, max_retries, error string）。即使最终失败，调用方也能通过流看到重试历史。

### OpenAiCompatibleProvider

**职责**：HTTP 客户端，实现完整的 LlmProvider trait，支持 OpenAI Chat Completions API 格式。

能力：
- 工具调用（`complete_with_tools` / `stream_complete_with_tools`）
- 推理模式（`ThinkingConfig` 映射到 provider 的推理参数）
- 流式响应（SSE）
- 自定义 HTTP header

错误映射：
```
HTTP 401/403 → LlmError::AuthFailed
HTTP 404 + body 包含 "model" → LlmError::ModelNotAvailable
HTTP 429 → LlmError::RateLimited（带 retry-after）
其他非 2xx → LlmError::RequestFailed
```

**注意**：目前只实现了 OpenAI-compatible 一种 provider。添加新 provider 需要在 `manager.rs:build_provider_with_model` 的 match 中添加分支。

## 约束

- ProviderManager 依赖 `LlmProviderRepository`（来自 argus-protocol）。Repository 的具体实现（数据库、文件等）由下游提供。
- API key 加密由 `argus-crypto` 处理（EncryptedSecret）。ProviderManager 在构建 provider 时调用 `.expose_secret()` 解密。
- 每个 provider 实例都是 `Arc<dyn LlmProvider>`，可在多线程间共享。

## 扩展点

**添加新 provider 类型**：
1. 在 `src/providers/` 实现 `LlmProvider` trait
2. 在 `src/providers/mod.rs` 导出
3. 在 `manager.rs:build_provider_with_model` 的 match 中添加分支
4. 在 `LlmProviderKind` 枚举（argus-protocol）中添加变体

**自定义重试策略**：RetryProvider 目前硬编码指数退避。如需自定义，实现自己的装饰器包装 LlmProvider。

## 下游依赖

```
argus-repository  — LlmProviderRepository 实现
argus-session     — 通过 ProviderManager 获取 provider
argus-turn        — 直接使用 provider（非 Manager 路径）
```
