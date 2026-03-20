# Argus-Tool 工具注册表

> 特性：工具注册表，基于 DashMap 提供 NamedTool 工具的注册和查找。

## 模块结构

```
src/
├── lib.rs           # 公共 API、ToolManager
├── glob.rs          # GlobTool：文件 glob 匹配
├── grep.rs          # GrepTool：内容搜索
├── http.rs          # HttpTool：HTTP 请求
├── read.rs          # ReadTool：文件读取
└── shell.rs         # ShellTool：Shell 执行
```

## 核心概念

### 1. NamedTool Trait

**NamedTool** 定义工具接口（来自 `argus-protocol`）：

```rust
pub trait NamedTool: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> ToolDefinition;
    fn risk_level(&self) -> RiskLevel;
    async fn execute(&self, args: ToolInput) -> Result<ToolOutput, ToolError>;
}
```

### 2. ToolManager

**ToolManager** 管理工具注册：

```rust
pub struct ToolManager {
    tools: DashMap<String, Arc<dyn NamedTool>>,  // 线程安全的工具注册表
}
```

**关键方法**：
- `register(tool)`：注册工具
- `get(name)`：获取工具
- `list_definitions()`：获取所有工具定义（供 LLM 使用）
- `execute(name, args)`：执行工具
- `get_risk_level(name)`：获取工具风险等级

## 内置工具

| 工具 | 风险等级 | 说明 |
|------|---------|------|
| `glob` | Medium | 文件路径模式匹配 |
| `grep` | Medium | 文件内容搜索 |
| `http` | Medium | HTTP 请求 |
| `read` | Low | 文件读取 |
| `shell` | Critical | Shell 命令执行 |

## 公共 API

```rust
use argus_tool::{ToolManager, GlobTool, ShellTool};
use argus_protocol::risk_level::RiskLevel;

// 创建 Manager
let manager = ToolManager::new();

// 注册工具
manager.register(Arc::new(GlobTool::new()));
manager.register(Arc::new(ShellTool::new()));

// 获取工具定义（供 LLM 使用）
let definitions = manager.list_definitions();

// 执行工具
let result = manager.execute("glob", serde_json::json!({
    "pattern": "**/*.rs"
})).await?;
```

## 依赖关系

### 上游依赖
- `argus-protocol`：NamedTool trait、RiskLevel

### 下游消费者
- `argus-turn`：执行工具调用
- `argus-thread`：管理工具生命周期

## 设计原则

### 1. 工具注册表
- DashMap 提供线程安全的并发访问
- 工具按名称注册，支持覆盖

### 2. 风险等级
- 每个工具有对应的风险等级
- 高风险工具（如 shell）需要审批
