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

## Linux 编译与部署

Linux 部署以 `argus-server` + `apps/web` 为目标，默认安装到 `/opt/arguswing`，使用 `arguswing.service` 运行。部署机需要 Rust/Cargo、pnpm、PostgreSQL 和 systemd；Nginx 只在选择反代模式时需要。

部署前先准备这些值：

- `DEPLOY_MODE`：`server` 表示 Rust server 同时托管前端；`nginx` 表示 Nginx 托管前端并反代 API
- `ARGUS_SERVER_ADDR`：server-hosted 模式的监听地址，默认 `0.0.0.0:3010`
- `DATABASE_URL`：PostgreSQL URL，例如 `postgres://argus:<password>@127.0.0.1:5432/argus`
- OAuth 配置：是否启用 OAuth、`client_id`、`client_secret`、`redirect_uri`
- 可选路径：`INSTALL_DIR=/opt/arguswing`、`ETC_DIR=/etc/arguswing`

编译后端和前端：

```bash
make linux-build
```

安装 server-hosted 模式：

```bash
sudo make linux-deploy DEPLOY_MODE=server \
  ARGUS_SERVER_ADDR=0.0.0.0:3010 \
  DATABASE_URL='postgres://argus:argus_dev@127.0.0.1:5432/argus_dev'
```

安装 Nginx 模式：

```bash
sudo make linux-deploy DEPLOY_MODE=nginx \
  DATABASE_URL='postgres://argus:argus_dev@127.0.0.1:5432/argus_dev'
sudo nginx -t
sudo systemctl reload nginx
```

部署脚本会创建系统用户/组、安装二进制和前端文件、写入 `/etc/arguswing/arguswing.toml`，并执行 `daemon-reload`、`enable` 和 `restart`。默认配置会先关闭 OAuth；生产环境请编辑 TOML：

```bash
sudoedit /etc/arguswing/arguswing.toml
```

如果字段包含 secret，不要把明文长期留在 TOML。先用同一个配置文件里的 `[crypto].master_key_path` 加密：

```bash
sudo /opt/arguswing/bin/argus-server \
  --config /etc/arguswing/arguswing.toml \
  config encrypt --value 'oauth-client-secret'
```

命令会输出：

```toml
{ encrypted = "...", nonce = "..." }
```

把输出粘到对应字段，例如：

```toml
[auth.oauth]
enabled = true
client_id = "your-client-id"
client_secret = { encrypted = "...", nonce = "..." }
redirect_uri = "https://argus.example.test/auth/callback"
```

`database.url` 如果包含密码，也可以用同样方式加密整个 PostgreSQL URL：

```toml
[database]
url = { encrypted = "...", nonce = "..." }
```

常用检查命令：

```bash
systemctl status arguswing.service
journalctl -u arguswing.service -f
curl http://127.0.0.1:3010/api/v1/health
```

默认路径：

- 程序：`/opt/arguswing/bin/argus-server`
- 前端：`/opt/arguswing/web`
- 配置：`/etc/arguswing/arguswing.toml`
- 主密钥：`/etc/arguswing/master.key`
- agent/thread traces：`/opt/arguswing/traces`
- 服务日志：默认进 journald；设置 `[logging].file_path` 后写入指定文件

务必同时备份 `arguswing.toml` 和 `master.key`。只备份 TOML 不够；丢失 `master.key` 后，TOML 中已有的密文 secret 无法解密。

管理员权限直接在 PostgreSQL `users` 表配置。用户第一次访问 `/api/v1/bootstrap` 后会创建/更新 `users` 记录，响应里的 `current_user.id` 即内部 UUID；授权管理员：

```sql
UPDATE users SET is_admin = TRUE WHERE id = '<current_user.id>';
```

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
