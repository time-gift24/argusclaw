# Argus-Template

> 特性：agent 模板管理与 builtin agents seed，统一维护 `AgentRecord` 的可持久化来源与 `subagent_names` 配置。

## 作用域

- 本文件适用于 `crates/argus-template/` 及其子目录。

## 核心职责

- `TemplateManager` 提供模板的 upsert / get / list / delete
- 从根目录 `agents/*.toml` 经 `generated_agents.rs` 嵌入后 seed builtin agents，允许没有任何 builtin agent
- 解析 TOML 中的 `subagent_names`，并在删除前做名称级引用校验

## 关键模块

- `src/manager.rs`
- `src/config.rs`
- `src/generated_agents.rs`

## 公开入口

- `TemplateManager`
- `AgentRecord`

## 修改守则

- builtin agent 定义来自根目录 `agents/*.toml` 与 `generated_agents.rs`，不要在运行时散落默认值
- 删除模板前必须尊重引用阻塞规则
- 模板数据模型应紧贴 `argus_protocol::AgentRecord`，不要派生第二套配置格式
- 子代理关系是基于 display name 的平铺配置，不要在这里恢复 parent-child 持久化
