# Argus-Approval 审批系统

> 特性：审批系统，通过 Hook 机制在危险工具执行前请求人工批准。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
├── hook.rs          # ApprovalHook：集成到 Turn 的 Hook
├── manager.rs       # ApprovalManager：审批请求管理
├── policy.rs        # ApprovalPolicy：审批策略配置
├── runtime_allow.rs # RuntimeAllowList：运行时允许列表
└── error.rs        # ApprovalError
```

## 核心概念

### 1. ApprovalHook

**ApprovalHook** 集成到 Turn 执行流程：

```rust
pub struct ApprovalHook {
    approval_manager: Arc<ApprovalManager>,  // 审批管理器
    policy: ApprovalPolicy,                   // 审批策略
    runtime_allow: Arc<RwLock<RuntimeAllowList>>,  // 运行时允许列表
    agent_id: String,
}
```

**执行流程**：
1. 检查运行时允许列表（用户已批准的工具）
2. 检查工具是否需要审批（基于策略）
3. 如需审批，创建审批请求并等待
4. 返回 `Continue` 或 `Block(reason)`

### 2. ApprovalManager

**ApprovalManager** 管理审批请求：

```rust
pub struct ApprovalManager {
    pending: DashMap<Uuid, PendingRequest>,      // 待处理请求
    policy: RwLock<ApprovalPolicy>,             // 审批策略
    event_tx: broadcast::Sender<ApprovalEvent>, // 事件广播
}
```

**关键特性**：
- 每个 agent 最多 5 个待处理请求
- 使用 oneshot channel 阻塞等待决策
- 广播事件通知前端

### 3. ApprovalPolicy

**ApprovalPolicy** 配置哪些工具需要审批：

```rust
pub struct ApprovalPolicy {
    pub require_approval_for_tools: Vec<String>,  // 需要审批的工具列表
    pub default_timeout_secs: u64,                // 默认超时时间
}
```

**默认策略**：`shell_exec` 等危险工具需要审批。

### 4. RuntimeAllowList

**RuntimeAllowList** 跟踪运行时已允许的工具：

- 允许特定工具（本次会话）
- 允许所有工具（本次会话）
- 应用重启后重置

## 公共 API

```rust
use argus_approval::{ApprovalManager, ApprovalPolicy, ApprovalHook, RuntimeAllowList};

// 创建 Manager 和 Policy
let policy = ApprovalPolicy::default();
let manager = Arc::new(ApprovalManager::new(policy.clone()));
let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));

// 创建 Hook 并注册到 HookRegistry
let hook = ApprovalHook::new(manager.clone(), policy, allow_list, "agent-1");
registry.register(HookEvent::BeforeToolCall, Arc::new(hook));
```

## 依赖关系

### 上游依赖
- `argus-protocol`：ApprovalRequest、HookHandler 等

### 下游消费者
- `argus-turn`：通过 HookRegistry 集成

## 设计原则

### 1. 阻塞式审批
- Tool 执行前阻塞，等待人工决策
- 超时后自动拒绝

### 2. 事件通知
- 广播 `ApprovalEvent` 通知前端
- 前端显示审批对话框
