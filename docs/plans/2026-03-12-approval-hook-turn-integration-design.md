# Approval Hook 与 Turn 集成设计

2026-03-12

## 目标

实现 ApprovalManager 与 Turn 执行的 Hook 系统集成，并创建 Shell 工具来验证整个流程。

## 背景
- ApprovalManager 已实现，但尚未与 Turn 执行流程集成
- Turn 执行通过 Hook 系统支持 `BeforeToolCall` 事件
- 默认策略下，所有工具都需要 approval（可配置）

## 设计

### ApprovalHook
实现 `HookHandler` trait，在 `BeforeToolCall` 事件中检查工具是否需要 approval:
- 检查 ApprovalPolicy 判断是否需要 approval
- 如果需要，创建 `ApprovalRequest` 并等待批准/拒绝/超时
- 根据结果返回 `Continue` 或 `Block`

### ShellTool
实现 `NamedTool` trait:
- 参数: `command` (必需), `timeout` (可选), `cwd` (可选)
- 使用 tokio::process::Command 执行命令
- 捕获 stdout/stderr 输出
- 返回退出码
- RiskLevel 设置为 `Critical`
- 默认需要 approval

### 集成点
- ApprovalHook 在 `BeforeToolCall` 时拦截
- 检查工具的 risk_level 或 policy
- 如果需要 approval, 调用 ApprovalManager
- CLI 提供 `--auto-approve` 标志用于自动批准测试
