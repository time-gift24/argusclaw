## 1. 向 argus-protocol 添加 plan 类型

- [x] 1.1 在 `crates/argus-protocol/src/plan.rs` 中创建 `StepStatus`、`PlanItemArg`、`UpdatePlanArgs`
- [x] 1.2 从 `crates/argus-protocol/src/lib.rs` 导出 plan 类型
- [x] 1.3 在 `plan.rs` 中添加 plan 类型序列化/反序列化测试

## 2. 向 Thread 添加 plan 字段

- [x] 2.1 向 `Thread` 结构体添加 `plan: Arc<RwLock<Vec<serde_json::Value>>>` 字段
- [x] 2.2 更新 `ThreadBuilder` 的 build 方法，用空的 `Vec` 初始化 plan
- [x] 2.3 添加 `pub fn plan(&self) -> &Arc<RwLock<Vec<serde_json::Value>>>` getter
- [x] 2.4 更新 `types.rs` 中的 `ThreadInfo` — 添加 `plan_item_count: usize` 字段
- [x] 2.5 更新 `Thread::info()` 以包含 plan 条目数

## 3. 创建 UpdatePlanTool

- [x] 3.1 在 `crates/argus-thread/src/plan_tool.rs` 中创建实现 `NamedTool` 的 `UpdatePlanTool` 结构体
- [x] 3.2 从 `crates/argus-thread/src/lib.rs` 导出 `UpdatePlanTool`
- [x] 3.3 在 `plan_tool.rs` 中为 `UpdatePlanTool` 添加单元测试

## 4. 将 UpdatePlanTool 接入 Turn 执行流程

- [x] 4.1 在 `execute_turn_streaming` 中，将 `Arc::new(UpdatePlanTool::new(self.plan.clone()))` push 到 tools `Vec`
- [x] 4.2 确认 `NamedTool` 导入包含 `UpdatePlanTool`

## 5. 验证

- [x] 5.1 运行 `cargo fmt`
- [x] 5.2 运行 `cargo clippy --all-targets`
- [x] 5.3 运行 `cargo test --all`
- [x] 5.4 运行 `prek`
