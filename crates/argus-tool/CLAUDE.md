# Argus-Tool

> 特性：`ToolManager` 与内置 filesystem、shell、browser、scheduler、clap tools。

## 核心职责

- `ToolManager` 负责注册、发现、执行与风险查询
- 内置文件工具：`glob`、`grep`、`read`、`list_dir`、`write_file`、`apply_patch`
- 执行与网络工具：`shell`、`http`
- 编排工具：`scheduler`
- 浏览器工具：`ChromeTool`
- CLI 适配：`ClapTool`

## 关键模块

- `src/lib.rs`
- `src/scheduler.rs`
- `src/clap_tool.rs`
- `src/chrome/*`
- `src/path_utils.rs`

## 公开入口

- `ToolManager`
- 各内置 tool 类型
- `SchedulerBackend`
- `ClapTool`、`ClapExecutor`

## 修改守则

- tool schema、风险等级与执行语义必须在实现里自洽，不能只靠 prompt 约定
- 任何会读写文件或执行命令的工具都要经过路径校验或明确的安全边界
- `ClapTool` 的 discoverability / alias / hide 规则见同目录 `AGENTS.md`
- 对外返回优先直接序列化底层库类型或原始 `serde_json::Value`；不要新增包装型 `*Summary` 输出。如果某能力只能靠自定义摘要才能暴露，优先不要暴露。
- `NamedTool::execute` 的成功返回必须先建模为 `#[derive(Serialize)]` 的 typed response，再通过统一 helper 序列化成 `serde_json::Value`；不要在成功路径直接手写 `json!({ ... })`。
- tool 的实现细节不要直接构造/返回 `ToolError`；每个 tool 先定义自己的 error 类型，在 `NamedTool` 边界通过 `From` / `Into` 统一转换。
- `scheduler` 与 `clap_tool` 仍有联合返回/动态返回场景，后续改动时按上述 typed response 与 tool-local error 规则单独收口。
