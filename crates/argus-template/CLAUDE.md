# Argus-Template

> 特性：agent 模板管理与 builtin agents seed，统一维护 `AgentRecord` 的可持久化来源。

## 核心职责

- `TemplateManager` 提供模板的 upsert / get / list / delete
- 从 `generated_agents.rs` 中的嵌入式 TOML seed builtin agents
- 维护 subagent 绑定与删除前引用校验

## 关键模块

- `src/manager.rs`
- `src/config.rs`
- `src/generated_agents.rs`

## 公开入口

- `TemplateManager`
- `AgentRecord`

## 修改守则

- builtin agent 定义来自嵌入式 TOML 与 `generated_agents.rs`，不要在运行时散落默认值
- 删除模板前必须尊重引用阻塞规则
- 模板数据模型应紧贴 `argus_protocol::AgentRecord`，不要派生第二套配置格式
