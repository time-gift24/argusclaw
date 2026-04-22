# Desktop Tauri Bridge

> 特性：Tauri Rust 桥接层，负责 tracing、invoke commands、事件订阅与桌面端启动。

## 作用域

- 本文件适用于 `crates/desktop/src-tauri/` 及其子目录。
- 如果需要跨到前端目录协作，再回看上层 `crates/desktop/AGENTS.md`。

## 核心职责

- `src/lib.rs`：初始化 tracing / tokio runtime / `ArgusWing`，注册 invoke handler
- `src/commands.rs`：前端与核心 facade 之间的命令桥
- `src/events.rs`：桌面事件载荷映射
- `src/subscription.rs`：thread subscription 复用与转发

## 依赖边界

- 上游依赖 `argus-wing`、`argus-protocol`、`argus-tool`
- 不在这里实现核心业务逻辑；业务能力尽量下沉到 `argus-wing` 或更低层 crate

## 修改守则

- 新增 command 时保持请求 / 响应可序列化，并同步前端绑定
- 优先复用 `ArgusWing` 的公开 API，不要直接绕过 facade 调底层仓储
- 涉及订阅和线程事件时，先确认桌面侧与前端事件名称、载荷结构一致
