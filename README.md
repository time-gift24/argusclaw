# ArgusClaw

ArgusClaw 是一个以 Rust workspace 为中心的 AI Agent 运行时，核心包含线程化对话执行、后台 job thread pool、工具系统、MCP 运行时，以及基于 Tauri 的桌面端。

## 快速开始

```bash
cargo install prek
prek install

cargo test
cargo deny check
prek
```

## 桌面端开发

```bash
cd crates/desktop
pnpm install
pnpm tauri dev
```

桌面端由两部分组成：

- `crates/desktop`：React 19 + Vite 8 前端
- `crates/desktop/src-tauri`：Tauri Rust bridge，负责 command / subscription / bootstrapping

## 工作区结构

| 路径 | 角色 |
| --- | --- |
| `crates/argus-protocol` | 核心类型与跨 crate trait，叶子模块 |
| `crates/argus-repository` | 唯一允许编写 SQL 的持久化层 |
| `crates/argus-crypto` | 密钥来源与凭证加解密 |
| `crates/argus-auth` | 账号与 token 包装 provider |
| `crates/argus-llm` | provider 管理、OpenAI-compatible provider、retry |
| `crates/argus-tool` | 工具注册表与内置 filesystem / shell / browser / scheduler tools |
| `crates/argus-agent` | thread-owned turn runtime、compact、trace、plan |
| `crates/argus-job` | 后台 job 调度、恢复与 runtime pool 解耦 |
| `crates/argus-session` | session 聚合、thread 恢复、scheduler backend |
| `crates/argus-template` | agent 模板管理与 builtin agents seed |
| `crates/argus-mcp` | MCP server runtime、supervision、tool adapter |
| `crates/argus-wing` | 面向应用层的 facade，组合所有子系统 |
| `crates/argus-test-support` | 测试辅助 provider 与 harness |
| `crates/desktop` | 桌面端 UI 与前端测试 |

## 文档约定

- 根目录与各 crate 维护本地 `AGENTS.md`，记录职责边界、修改守则与补充不变量。
- 进入更深目录工作时，以更近的 `AGENTS.md` 为准。
