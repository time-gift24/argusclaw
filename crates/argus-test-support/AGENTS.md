# Argus-Test-Support

> 特性：为 LLM、agent 与 retry 测试提供 mock providers 和辅助类型。

## 作用域

- 本文件适用于 `crates/argus-test-support/` 及其子目录。

## 核心职责

- 提供稳定、可组合的测试 provider
- 覆盖“始终失败”“间歇失败”等常见异常路径
- 让上层 crate 不必在各自测试里重复造轮子

## 关键模块

- `src/providers/always_fail.rs`
- `src/providers/intermittent.rs`
- `src/providers/mod.rs`

## 公开入口

- `AlwaysFailProvider`
- `IntermittentFailureProvider`

## 修改守则

- 只服务测试场景，不要让生产代码依赖这个 crate
- mock 行为要保持可预测，避免引入隐式随机性
- 若新增测试 provider，优先覆盖上层确实重复出现的失败模式
