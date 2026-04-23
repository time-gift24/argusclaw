# Argus-Server

> 特性：基于 axum 的实例级管理面 transport，只通过 ArgusWing facade 暴露 phase 1 REST API。

## 作用域

- 本文件适用于 `crates/argus-server/` 及其子目录。

## 核心职责

- 启动并持有 `ArgusWing`
- 暴露 health / bootstrap / providers / templates / mcp / settings 的 REST API
- 负责 HTTP 请求校验、序列化与错误映射

## 修改守则

- 不要绕过 `ArgusWing` 直接访问下层 manager 或 repository
- 首阶段只做实例级管理 API，不扩展 chat / thread / SSE
- 路由保持窄接口，避免把 desktop 命令面直接平移成大而全的 server surface
