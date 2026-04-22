# Argus-Auth

> 特性：账号存储与 token 包装 provider，连接 account repository、credential crypto 与 LLM 认证上下文。

## 作用域

- 本文件适用于 `crates/argus-auth/` 及其子目录。

## 核心职责

- `AccountManager` 管理账号信息、登录状态与凭证落库
- `TokenSource` / `TokenLLMProvider` 为现有 `LlmProvider` 增加 token 注入能力
- `TokenConfig` / `TokenContext` 封装 token endpoint 与运行时依赖

## 关键模块

- `src/account.rs`：`AccountManager`、`UserInfo`
- `src/token.rs`：`AccountTokenSource`、`SimpleTokenSource`、`TokenLLMProvider`
- `src/error.rs`：`AuthError`

## 公开入口

- `AccountManager`
- `AccountRepository`
- `TokenLLMProvider`
- `TokenConfig`、`TokenContext`

## 依赖边界

- 上游依赖：`argus-crypto`、`argus-protocol`、`argus-repository`
- 下游消费者：`argus-llm`、`argus-wing`

## 修改守则

- 凭证只能经过 repository + crypto 流程持久化，不要引入明文缓存落盘
- token 注入逻辑留在本 crate；不要把认证细节散落到 provider 实现里
- 认证失败、刷新失败等错误优先在这里分类，再向上层暴露稳定语义
