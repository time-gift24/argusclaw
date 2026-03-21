## 1. FilePlanStore 实现

- [x] 1.1 创建 `crates/argus-thread/src/plan_store.rs`，实现 `FilePlanStore` 结构体，包含 `store: Arc<RwLock<Vec<Value>>>` 和 `path: PathBuf`
- [x] 1.2 实现 `FilePlanStore::new(trace_dir: PathBuf, thread_id: &ThreadId)` 创建 plan.json 路径并尝试从文件恢复
- [x] 1.3 实现 `FilePlanStore::write(&self, plan: Vec<Value>)` 写入内存和文件
- [x] 1.4 实现 `FilePlanStore::store(&self) -> Arc<RwLock<Vec<Value>>>` 返回内存引用
- [x] 1.5 在 `crates/argus-thread/src/lib.rs` 中导出 `pub mod plan_store`
- [x] 1.6 验证：编译通过，无 clippy 警告

## 2. Thread 集成 FilePlanStore

- [x] 2.1 在 `thread.rs` 中引入 `use super::plan_store::FilePlanStore`
- [x] 2.2 Thread struct 新增 `plan_store: FilePlanStore` 字段（替换现有的 `plan: Arc<RwLock<Vec<Value>>>`）
- [x] 2.3 ThreadBuilder 新增 `plan_store` 参数，从 config 或直接从 trace_config 获取路径
- [x] 2.4 `Thread::info()` 中的 `plan_item_count` 改为 `self.plan_store.store().read().unwrap().len()`
- [x] 2.5 `execute_turn_streaming()` 中 UpdatePlanTool 改为从 `self.plan_store.store()` 获取引用
- [x] 2.6 验证：`cargo build -p argus-thread` 通过

## 3. UpdatePlanTool 改为使用 FilePlanStore

- [x] 3.1 修改 `plan_tool.rs` 中 `UpdatePlanTool` 的 `execute()` 方法，改为调用 `FilePlanStore::write()`
- [x] 3.2 `execute()` 中同时将 `Vec<UpdatePlanArgs>` 序列化为 `Vec<Value>` 写入
- [x] 3.3 单元测试更新：保持行为不变，仅存储后端改变
- [x] 3.4 验证：`cargo test -p argus-thread plan_tool` 通过

## 4. 前端 Plan 类型定义

- [x] 4.1 创建 `crates/desktop/lib/types/plan.ts`，定义 `PlanItem` 类型（`{ step: string; status: "pending" | "in_progress" | "completed" }`）
- [x] 4.2 导出 `PlanItem` 类型

## 5. ChatSessionState 增加 plan 状态

- [x] 5.1 在 `chat-store.ts` 的 `ChatSessionState` 接口中新增 `plan: PlanItem[] | null`
- [x] 5.2 初始化为 `null`
- [x] 5.3 在 `_handleThreadEvent()` 中处理 `tool_completed` 事件：当 `event.tool_name === "update_plan"` 时，从 `event.result.plan` 解析并写入 state
- [x] 5.4 验证：TypeScript 编译通过

## 6. PlanPanel 组件

- [x] 6.1 创建 `crates/desktop/components/chat/plan-panel.tsx`
- [x] 6.2 接收 `plan: PlanItem[]` 作为 prop
- [x] 6.3 渲染 Header：`"Plan (completed/total)"` + 折叠按钮
- [x] 6.4 渲染步骤列表，每个步骤根据 status 显示对应图标（pending 圆圈 / in_progress 进行中 / completed 勾选）
- [x] 6.5 内部状态管理展开/折叠，默认为展开
- [x] 6.6 验证：编译通过，组件渲染正常

## 7. PlanPanel 嵌入 ThreadViewport

- [x] 7.1 在 `components/assistant-ui/thread.tsx` 中引入 PlanPanel
- [x] 7.2 从 ChatSessionState 读取 `plan` 状态
- [x] 7.3 当 `plan !== null` 时在 ThreadViewport 顶部渲染 `<PlanPanel plan={plan} />`
- [x] 7.4 验证：进入有 plan 的 Thread 时面板显示，进入无 plan 的 Thread 时面板不渲染

## 8. plan_item_count 集成

- [x] 8.1 确认 `crates/desktop/lib/types/chat.ts` 中 `ThreadSnapshotPayload` 的 `ThreadInfo` 类型包含 `plan_item_count`
- [x] 8.2 如果缺失，在 `ThreadInfo` 类型定义中添加 `plan_item_count: number`
- [x] 8.3 验证：`get_thread_snapshot` 调用后返回的数据包含 plan_item_count
