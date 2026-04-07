# Argus-Server Axum HTTP 服务

> 特性：基于 axum 的 HTTP 服务器，提供 OAuth2 登录、dev 模拟认证和面向终端用户的聊天 API。

## 快速启动

```bash
# 1. 确保 PostgreSQL 13.22 正在运行
createdb argus_server

# 2. 设置环境变量
export ARGUS_DATABASE_URL="postgres://localhost/argus_server"
export ARGUS_LISTEN_ADDR="0.0.0.0:3000"

# 3. 运行 schema migration（自动执行于启动时）
# 或者手动: psql -d argus_server -f crates/argus-repository/src/postgres/schema.sql

# 4. 启动
cargo run -p argus-server

# 5. 浏览器访问 http://localhost:3000/auth/login
#    Dev OAuth2 表单允许选择测试账号登录
```

## API 概览

| 路由 | 方法 | 说明 |
|------|------|------|
| `/auth/login` | GET | 发起 OAuth2 登录 |
| `/dev-oauth/authorize` | GET | Dev 模式授权表单 |
| `/dev-oauth/authorize` | POST | 提交授权 |
| `/auth/callback` | GET | OAuth2 回调 |
| `/auth/logout` | POST | 登出 |
| `/api/me` | GET | 当前用户信息 |
| `/api/agents` | GET | 列出已启用 agent |
| `/api/sessions` | POST | 创建会话 |
| `/api/sessions` | GET | 列出用户会话 |
| `/api/sessions/:id/threads` | GET | 列出会话线程 |
| `/api/threads/:id` | GET | 线程详情 |
| `/api/threads/:id/messages` | POST | 发送消息 |
| `/api/threads/:id/cancel` | POST | 取消工作 |
| `/api/threads/:id/events` | GET | SSE 事件流 |

## 与 Desktop 的关系

- Desktop 使用 `argus-wing` 的 `ArgusWing` facade，不经过 server crate
- Server 使用 `argus-session::UserChatServices` 做 user-aware 隔离
- 两者共享核心 runtime（session/thread/turn、provider、tool）
- Desktop 不受 OAuth2 影响，保持原有 login 流程

## 测试

```bash
# Unit + integration（不需要 PG）
cargo test -p argus-server

# PostgreSQL integration tests（需要 ARGUS_TEST_PG_URL）
ARGUS_TEST_PG_URL="postgres://localhost/argus_test" \
  cargo test -p argus-repository --features postgres
```
