# ArgusClaw 开发指南

## 先读哪里

- 根目录规则适用于整个仓库。
- 进入 `crates/*` 或更深目录时，优先遵循最近的 `CLAUDE.md` / `AGENTS.md`。
- `AGENTS.md` 主要负责提醒代理先读 `CLAUDE.md`；更细的设计边界与不变量写在 `CLAUDE.md`。

## 构建与检查

务必先完成：

```bash
cargo install prek && prek install
```

常用命令：

```bash
prek
cargo test
cargo deny check

cd crates/desktop
pnpm install
pnpm tauri dev
```

## 设计与沟通原则

- YAGNI：不要为尚未确认的场景提前扩展
- KISS：优先直接、可恢复、可读的实现
- DRY：避免横向复制逻辑和规则
- 先澄清目标，再实现；不要默认用户已经把目标说完整
- 解释问题时追根到职责边界或状态模型，不要只描述补丁
- 输出保持简洁，只覆盖变更点、风险点和验证结果

## 开始改动前

**禁令（极其重要）**

- ❌ 禁止直接在 `main` 分支所在工作区修改文件
- ✅ 必须在 `.worktrees/` 下的独立分支里工作

推荐流程：

```bash
git worktree add .worktrees/<branch-name> -b codex/<branch-name>
```

## 代码风格

- 跨模块导入优先使用 `crate::`；测试和局部模块引用使用 `super::`
- 除对外稳定 API 外，避免无必要的 `pub use` 重导出
- 生产代码中不用 `.unwrap()` / `.expect()`；测试代码可酌情使用
- 错误类型优先用 `thiserror`
- 优先强类型而不是裸字符串
- 单个函数保持单一职责，提取可复用辅助函数
- 只在逻辑不明显时写注释

## 全局架构边界

| 模块 | 角色 | 约束 |
| --- | --- | --- |
| `argus-protocol` | 核心共享类型、trait、事件与安全边界 | 叶子模块，不写业务编排 |
| `argus-repository` | Repository trait + SQLite 实现 | **仓库里唯一允许写 SQL 的地方** |
| `argus-crypto` | 凭证加解密与 key source | 保持与业务逻辑解耦 |
| `argus-auth` | 账号管理与 token 包装 provider | 认证状态不要泄漏到无关 crate |
| `argus-llm` | Provider manager、OpenAI-compatible provider、retry | provider 细节封装在此处 |
| `argus-tool` | ToolManager 与内置 tools | 风险分级、schema、执行边界都在此维护 |
| `argus-agent` | thread-owned turn runtime、compact、plan、trace | thread/turn 的事实来源和结算规则集中于此 |
| `argus-job` | 后台 job 生命周期与统一 thread pool | job child thread 必须受池统一管理 |
| `argus-session` | session 聚合、thread 恢复、scheduler backend | 负责把 agent/job/tool 组合成会话层语义 |
| `argus-template` | agent template 管理与 builtin seed | 模板不直接做运行时编排 |
| `argus-mcp` | MCP server runtime、supervision、tool adapter | MCP 连接与发现逻辑只放这里 |
| `argus-wing` | 应用 facade | 面向桌面桥接层的唯一稳定入口 |
| `crates/desktop` | React 前端 | 通过 Tauri command 与 facade 交互 |
| `crates/desktop/src-tauri` | Tauri Rust bridge | 不要在这里堆核心业务逻辑 |

## 关键约束

- 所有持久化依赖通过 trait / `Arc<dyn ...>` 注入，不要把 repository 具体实现扩散到上层
- `desktop` 侧只通过 `argus-wing` 暴露的 API 访问核心系统
- crate 级 `CLAUDE.md` 顶部保留一句话 `> 特性：...`
- 新增或调整 crate 文档时，记得同步旁边的 `AGENTS.md`
