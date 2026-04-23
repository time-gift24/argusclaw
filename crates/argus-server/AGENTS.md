# Argus-Server

> 特性：基于 axum 的实例级管理面与运行状态 transport，在 server 内私有装配 ServerCore。

## 作用域

- 本文件适用于 `crates/argus-server/` 及其子目录。

## 核心职责

- 启动并持有 `ServerCore`
- 在 `ServerCore` 内装配 provider、template、MCP、session、job、thread-pool、tool、auth 等 server 运行组件
- 暴露 health / bootstrap / providers / templates / mcp / settings / runtime / runtime/events / tools 管理 API
- Phase 5 起允许暴露 server-only chat REST API：sessions / threads / messages / send / cancel / rename / model binding / snapshot / activate
- 负责 HTTP 请求校验、序列化与错误映射

## 修改守则

- `argus-server` 不依赖 `argus-wing`；两者是平等的应用入口
- 下层 manager / repository 的直接装配只允许集中在 `ServerCore`，route handler 只调用 `ServerCore` 暴露的窄方法
- chat / thread / message API 仅按 Phase 5 的 server-only REST 边界扩展；不改 desktop 主流程，不做 web chat UI，不加 thread event SSE
- settings 由 `ServerCore` 通过 repository 持久化，默认 `instance_name = "ArgusWing"`
- 路由保持窄接口，避免把 desktop 命令面直接平移成大而全的 server surface
