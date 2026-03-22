## ADDED Requirements

### Requirement: MCP 服务器 HTTP headers 配置 UI

当 MCP 服务器类型为 HTTP 时，表单 SHALL 显示 HTTP headers 配置区域，允许用户添加、修改和删除 HTTP 请求头。

#### Scenario: 显示 headers 输入区域

- **WHEN** 用户选择 server_type 为 "Http"
- **THEN** 表单显示 "HTTP Headers" 配置区域，包含 "Add Header" 按钮

#### Scenario: 添加 header

- **WHEN** 用户点击 "Add Header" 按钮
- **THEN** 表单显示新的 header 行，包含 Key 和 Value 两个输入框

#### Scenario: 删除 header

- **WHEN** 用户点击 header 行上的删除按钮
- **THEN** 该 header 行被移除

#### Scenario: Stdio 类型不显示 headers

- **WHEN** 用户选择 server_type 为 "Stdio"
- **THEN** HTTP Headers 配置区域被隐藏

### Requirement: Headers 数据结构

HTTP headers SHALL 以 `Record<string, string>` 格式存储，空的 key-value 对应在提交时忽略。

#### Scenario: 提交表单时过滤空值

- **WHEN** 用户提交表单，包含空的 header key 或 value
- **THEN** 空的 header 被过滤掉，不包含在提交的 payload 中
