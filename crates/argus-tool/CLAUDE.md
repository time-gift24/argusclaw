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
