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

## Linux 部署

Linux 部署以 `argus-server` + `apps/web` 为目标，默认安装到 `/opt/arguswing`，使用 `arguswing.service` 运行。部署机需要已安装 Rust/Cargo、pnpm 和 systemd。

推荐先使用 server-hosted 模式，由 Rust server 同时提供 API 和前端静态文件：

```bash
sudo make linux-deploy DEPLOY_MODE=server
```

该命令会构建后端和前端、创建缺失的 `arguswing` 系统用户/组、安装文件、写入 systemd 配置，并执行 `daemon-reload`、`enable` 和 `restart`。默认监听 `0.0.0.0:3010`，可直接访问 `http://<server-ip>:3010`。

如果希望由 Nginx 托管前端并反代 API：

```bash
sudo make linux-deploy DEPLOY_MODE=nginx
sudo nginx -t
sudo systemctl reload nginx
```

Nginx 模式下 `argus-server` 默认监听 `127.0.0.1:3010`，部署脚本会安装或暂存 `deploy/nginx/arguswing.conf`。

常用检查命令：

```bash
systemctl status arguswing.service
journalctl -u arguswing.service -f
curl http://127.0.0.1:3010/api/v1/health
```

默认路径：

- 程序：`/opt/arguswing/bin/argus-server`
- 前端：`/opt/arguswing/web`
- 数据库：`/opt/arguswing/data/sqlite.db`
- traces：`/opt/arguswing/traces`
- 环境变量：`/etc/arguswing/arguswing.env`

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
| `crates/argus-wing` | desktop 侧应用 facade，组合桌面端所需子系统 |
| `crates/argus-server` | axum 管理面服务，私有装配 server 运行内核 |
| `crates/argus-test-support` | 测试辅助 provider 与 harness |
| `crates/desktop` | 桌面端 UI 与前端测试 |
| `apps/web` | Vue 3 + OpenTiny Vue 独立管理台 |

## 文档约定

- 根目录与各 crate 维护本地 `AGENTS.md`，记录职责边界、修改守则与补充不变量。
- 进入更深目录工作时，以更近的 `AGENTS.md` 为准。
