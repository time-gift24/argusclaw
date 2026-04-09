# Argus-Wing

> 特性：应用 facade，负责 bootstrapping managers / runtimes，并向桌面桥接层暴露稳定 API。

## 核心职责

- `ArgusWing::init()` / `with_pool()` 统一启动数据库、auth、provider manager、template manager、tool manager、job manager、MCP runtime、session manager
- 对外暴露 provider、agent template、session、thread、account、MCP 等应用层 API
- 保持桌面桥接层只依赖一个 facade，而不直接拼装底层 crate

## 关键模块

- `src/lib.rs`：`ArgusWing` 主体与公开 API
- `src/db.rs`：数据库路径解析
- `src/resolver.rs`：`ProviderManagerResolver`

## 修改守则

- 这里是组合层，不是业务实现层；复杂逻辑应下沉到更低层 crate
- `init()` 与 `with_pool()` 的装配顺序和能力要保持一致
- 新增对外 API 时，优先复用既有 manager 语义，避免在 facade 层发明第二套状态
