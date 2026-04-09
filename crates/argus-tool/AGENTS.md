# argus-tool AGENTS Guide

先熟悉同目录 `CLAUDE.md`。

以下内容补充 `ClapTool` 的设计细节和后续开发约束。

## ClapTool 是什么

`ClapTool` 的目标是把一个现成的 `clap::Command` 包装成 `NamedTool`，让 LLM 可以按“先发现、再执行”的方式调用 CLI。

它对外暴露的是统一 JSON 输入，而不是直接让模型拼 shell：

1. `{"action":"help"}`：列出可见子命令
2. `{"action":"help","subcommand":"install"}`：查看某个子命令参数
3. `{"action":"install","args":{...}}`：执行子命令

对应实现见 `src/clap_tool.rs`。

## 核心原理

### 1. 两阶段交互

`ClapTool` 不是一次性把整个 CLI 文档塞给模型，而是让模型先通过 `help` 了解结构，再带着结构化参数执行。这样能降低：

- 参数名猜错
- 调用隐藏子命令
- 把字符串值误当成 shell 片段

### 2. Schema 由 clap::Command 动态生成

`definition()` 会根据 `Command` 生成一个 `oneOf` schema：

- 一个 `help` 分支
- 每个可见子命令一个分支
- `action` 是判别字段
- `args` 会带上 `required`
- `args` 默认 `additionalProperties: false`
- 可见 alias 会写进 `action`/`subcommand` 的 schema

也就是说，`clap::Command` 是事实来源，tool schema 只是它的投影。

### 3. 只暴露“可见”表面

`ClapTool` 会过滤掉：

- `hide(true)` 的 subcommand
- `hide(true)` 的 argument

这些隐藏项既不会出现在 schema/help 中，也不应该作为 LLM API 的稳定入口。

如果某个子命令需要给人类 CLI 保留内部开关，但不希望 agent 使用，优先用 `hide(true)`，而不是只在 prompt 里约定“不许调用”。

### 4. 执行仍然走 clap 解析

`ClapTool` 并不手写一套参数绑定逻辑，它会把 JSON `args` 转成安全的 argv，再交回 `clap` 做真正解析。

这个设计的好处是：

- 必填校验仍由 `clap` 保证
- alias 解析仍由 `clap` 语义兜底
- `ArgMatches` 仍然是 executor 看到的唯一解析结果

### 5. 安全边界在 json_to_argv

`json_to_argv` 是 `ClapTool` 的关键安全层，当前约束包括：

- 只接受 object/null 作为 `args`
- 拒绝未知字段
- 选项值使用 `--flag=value` 形式，避免把值重新解析成另一个 flag
- 位置参数统一在 `--` 后追加
- flag/count/multi-value 会按 `clap` 的 action 语义转换

如果后续改这里，优先把它当作“参数绑定层”而不是“字符串拼接层”。

## ClapExecutor 的职责

`ClapExecutor` 负责消费已经通过 `clap` 校验的结果：

```rust
#[async_trait]
pub trait ClapExecutor: Send + Sync {
    async fn execute(
        &self,
        subcommand: &str,
        matches: &ArgMatches,
        tool_name: &str,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError>;
}
```

约束如下：

- `subcommand`：已经是解析后的子命令名，优先按 canonical name 写分发逻辑
- `matches`：从这里读取参数，不要重新解析 JSON
- `tool_name`：用于错误上下文
- `ctx`：如果需要线程上下文、pipe/control 通道，从这里拿，不要绕过 tool 层

换句话说，`ClapTool` 负责“把 LLM 输入变成安全的 clap 调用”，`ClapExecutor` 负责“把 clap 调用映射成业务行为”。

## 后续开发时怎么用

### 场景 1：把已有 clap CLI 暴露成 tool

推荐步骤：

1. 先定义好 `Command` 树
2. 用 `hide(true)` 隐藏不希望 LLM 发现的入口
3. 实现一个 `ClapExecutor`
4. `ClapTool::new(name, description, command, executor, risk)`
5. 注册到 `ToolManager`

### 场景 2：新增子命令

新增子命令时，优先检查 4 件事：

1. 这个子命令是否应该被 LLM 发现
2. 参数名是否适合 JSON key
3. 是否有 required/alias/hide 语义
4. executor 是否能只依赖 `ArgMatches` 完成分发

如果某个子命令需要大量 prompt 解释才能安全调用，通常说明它还不适合作为 `ClapTool` 暴露面。

### 场景 3：新增参数类型

如果是常见 `clap` action，先确认会不会影响：

- schema 类型推断
- `help` 输出
- `json_to_argv` 转换
- 执行侧的 `ArgMatches` 读取方式

任何新增 action/参数语义，都要同时更新这三层，而不是只改其中一层。

## 推荐测试清单

对 `ClapTool` 自身或基于它的新工具，至少覆盖这些测试：

- schema 中是否包含正确的 `oneOf` 分支
- required 参数是否写进 `args.required`
- hidden subcommand/arg 是否不会暴露
- alias 是否能通过 schema/help 被发现，并能执行
- positional 参数是否能正确转换
- `--value` 这类以连字符开头的字符串是否不会被误解析成 flag
- executor 是否能拿到 `ToolExecutionContext`

## 开发约定

- 优先把语义建模进 `clap::Command`，不要在 `ClapExecutor` 里补一层“隐式规则”
- 不要为了兼容模型输出，放宽 hidden/unknown 参数校验
- 如果 schema 与执行行为不一致，优先修 schema 生成，而不是在 prompt 里打补丁
- 新增功能时，先补回归测试，再改 `clap_tool.rs`

## 一个简单心智模型

可以把 `ClapTool` 看成三层：

1. `Command -> JSON Schema`
2. `JSON args -> safe argv`
3. `ArgMatches -> business executor`

后续改动时，先判断自己改的是哪一层，再检查是否会影响另外两层。
