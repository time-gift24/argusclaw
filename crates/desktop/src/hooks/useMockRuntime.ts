// crates/desktop/src/hooks/useMockRuntime.ts

import { useState, useCallback } from "react";
import {
  useExternalStoreRuntime,
  type ThreadMessageLike,
  type AppendMessage,
} from "@assistant-ui/react";

/**
 * 预定义的 Mock 响应
 */
const MOCK_RESPONSES = {
  default: `这是一个 Mock 响应示例。

\`\`\`typescript
const greeting: string = "Hello, World!";
console.log(greeting);
\`\`\`

支持 **Markdown** 格式，包括：
- 列表
- **粗体** 和 *斜体*
- \`行内代码\``,

  code: `这是一个代码示例：

\`\`\`rust
fn main() {
    println!("Hello from Rust!");
}

struct Message {
    content: String,
    role: String,
}
\`\`\`

\`\`\`python
def greet(name: str) -> str:
    return f"Hello, {name}!"

print(greet("ArgusClaw"))
\`\`\``,

  mermaid: `这是一个 Mermaid 图表：

\`\`\`mermaid
graph TD
    A[用户输入] --> B{验证}
    B -->|通过| C[处理请求]
    B -->|失败| D[返回错误]
    C --> E[调用 LLM]
    E --> F[返回结果]
\`\`\`

流程图展示了请求处理的完整流程。`,

  math: `这是一个数学公式示例：

$$E = mc^2$$

爱因斯坦的质能方程。

行内公式：$a^2 + b^2 = c^2$（勾股定理）

更复杂的公式：
$$
f(x) = \\int_{-\\infty}^{\\infty} \\hat{f}(\\xi) e^{2\\pi i \\xi x} d\\xi
$$`,
};

interface MockMessage {
  role: "user" | "assistant";
  content: string;
}

/**
 * 根据用户消息返回对应的 Mock 响应
 */
function getMockResponse(userMessage: string): string {
  const lowerMessage = userMessage.toLowerCase();

  if (lowerMessage.includes("代码") || lowerMessage.includes("code")) {
    return MOCK_RESPONSES.code;
  }
  if (lowerMessage.includes("图表") || lowerMessage.includes("mermaid") || lowerMessage.includes("流程")) {
    return MOCK_RESPONSES.mermaid;
  }
  if (lowerMessage.includes("数学") || lowerMessage.includes("math") || lowerMessage.includes("公式")) {
    return MOCK_RESPONSES.math;
  }

  return MOCK_RESPONSES.default;
}

/**
 * 将内部消息格式转换为 assistant-ui 的 ThreadMessageLike
 */
function convertMessage(message: MockMessage): ThreadMessageLike {
  return {
    role: message.role,
    content: [{ type: "text", text: message.content }],
  };
}

/**
 * Mock Runtime Hook
 * 提供 Mock 数据供 assistant-ui 使用
 */
export function useMockRuntime() {
  const [isRunning, setIsRunning] = useState(false);
  const [messages, setMessages] = useState<MockMessage[]>([]);

  const onNew = useCallback(async (message: AppendMessage) => {
    if (message.content[0]?.type !== "text") {
      throw new Error("Only text messages are supported");
    }

    const input = message.content[0].text;

    // 添加用户消息
    setMessages((prev) => [...prev, { role: "user", content: input }]);

    setIsRunning(true);

    // 模拟网络延迟
    await new Promise((resolve) => setTimeout(resolve, 500));

    // 获取 Mock 响应
    const aiResponse = getMockResponse(input);

    // 添加 AI 响应消息
    setMessages((prev) => [...prev, { role: "assistant", content: aiResponse }]);
    setIsRunning(false);
  }, []);

  return useExternalStoreRuntime({
    isRunning,
    messages,
    convertMessage,
    onNew,
  });
}
