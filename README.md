# ArgusWing

AI Agent 框架，支持 CLI 和桌面应用。

## 项目结构

```
argusclaw/
├── crates/
│   ├── argus-protocol/    # 核心类型（叶子模块）
│   ├── argus-session/     # 会话管理
│   ├── argus-thread/      # 线程管理
│   ├── argus-turn/        # 轮次执行
│   ├── argus-llm/        # LLM 抽象层
│   ├── argus-approval/    # 审批系统
│   ├── argus-tool/       # 工具注册表
│   ├── argus-repository/ # 持久化层
│   ├── claw/             # 核心库门面
│   ├── cli/              # CLI 前端
│   └── desktop/          # Tauri 桌面应用
```

## 编译 Desktop

### 前置要求

- **Rust** (via [rustup](https://rustup.rs/))
- **pnpm** (`npm install -g pnpm`)
- **macOS**: Xcode Command Line Tools

### 1. 安装前端依赖

```bash
cd crates/desktop
pnpm install
```

### 2. 开发模式

```bash
# 启动 Tauri 开发服务器（会自动启动 Next.js 前端）
pnpm tauri dev
```

### 3. 生产构建

```bash
# 构建前端 + Rust 后端
pnpm tauri build
```

产物输出到 `src-tauri/target/release/bundle/` 目录。

### 常见问题

**Q: 编译卡住不动**
- 首次编译需要下载 Rust 依赖，可能需要几分钟
- 确保网络通畅

**Q: 权限错误 (macOS)**
- 运行 `sudo xcode-select --reset` 重置 Xcode 路径

**Q: Windows 构建失败**
- 需要 Visual Studio Build Tools
- 确保安装 "Desktop development with C++" 工作负载

## CLI 开发

```bash
cargo build --release -p cli
./target/release/arguswing --help
```

## 开发检查

```bash
prek  # 静态检查（commit 前必须通过）
```
