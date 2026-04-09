# Argus-LLM

> 特性：provider 管理、OpenAI-compatible provider、retry decorator 与测试辅助。

## 核心职责

- `ProviderManager` 从 repository record 构造、缓存并测试 provider
- `providers/openai_compatible.rs` 负责 OpenAI-compatible 请求映射、流式响应与错误分类
- `RetryProvider` 为任意 `LlmProvider` 增加重试、退避与 retry 事件
- `test_utils.rs` 与 `src/bin/cli.rs` 提供测试和手动验证入口

## 关键模块

- `src/manager.rs`
- `src/providers/openai_compatible.rs`
- `src/retry.rs`
- `src/test_utils.rs`
- `src/bin/cli.rs`

## 公开入口

- `ProviderManager`
- `create_openai_compatible_provider`
- `OpenAiCompatibleConfig`、`OpenAiCompatibleFactoryConfig`
- `RetryProvider`、`RetryConfig`
- `create_test_retry_provider`

## 依赖边界

- 上游依赖：`argus-protocol`、`argus-auth`、`argus-crypto`、`argus-repository`
- 下游消费者：`argus-wing`、`argus-session`、`argus-job`

## 修改守则

- provider 专属协议映射放在 `providers/*`，不要把 provider if/else 散到 manager 或上层
- retry 行为必须保持与 `LlmStreamEvent::RetryAttempt` 语义一致
- secret 处理优先走 auth / repository / crypto 边界，不要在上层长期持有原始 key 字符串
