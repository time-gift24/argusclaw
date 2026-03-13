# Chat 页面替换：使用 assistant-ui

**日期**: 2026-03-12
**状态**: 设计阶段
**作者**: Claude

## 背景

当前 chat 页面只是一个占位符，显示"选择一个对话开始"。实际的 markdown 渲染在 `streamdown-dev` 页面中使用 Streamdown 库实现。

目标是使用 [assistant-ui](https://www.assistant-ui.com/) 提供的生产级聊天组件替换现有实现，同时移除 Streamdown 依赖。

## 目标

1. 用 assistant-ui 替换 chat 页面，提供 ChatGPT 风格的聊天体验
2. 移除 Streamdown 及相关包
3. 保留关键功能：代码高亮、Mermaid 图表、数学公式
4. 先使用 Mock 数据，后续接入 Tauri 后端

## 非目标

- 本次不实现后端集成（Tauri 命令）
- 不实现对话持久化
- 不实现多会话管理

## 架构设计

### 组件结构

```
App.tsx
└── ChatPage
    └── AssistantRuntimeProvider
        └── Thread
            ├── ThreadMessages (消息列表)
            │   └── MarkdownText (自定义 markdown 渲染)
            │       ├── CodeBlock (Shiki 代码高亮)
            │       ├── MermaidBlock (Mermaid 图表)
            │       └── MathBlock (KaTeX 数学公式)
            └── Composer (输入框)
```

### Runtime 选择

使用 `ExternalStoreRuntime`，因为：
- 后端是 Tauri + Rust，不是 Vercel AI SDK
- 需要自定义消息存储和发送逻辑
- Mock 阶段易于实现，后续可无缝切换到真实后端

## 文件变更

### 新增文件

| 文件路径 | 说明 |
|---------|------|
| `crates/desktop/src/components/chat/ChatPage.tsx` | 主聊天页面组件 |
| `crates/desktop/src/components/chat/markdown/MarkdownText.tsx` | 自定义 markdown 渲染组件 |
| `crates/desktop/src/components/chat/markdown/CodeBlock.tsx` | 代码高亮组件（迁移自旧 code-block.tsx） |
| `crates/desktop/src/components/chat/markdown/MermaidBlock.tsx` | Mermaid 图表渲染 |
| `crates/desktop/src/components/chat/markdown/MathBlock.tsx` | KaTeX 数学公式渲染 |
| `crates/desktop/src/hooks/useMockRuntime.ts` | Mock runtime hook |
| `crates/desktop/src/lib/chat-types.ts` | 聊天相关类型定义 |

### 删除文件

| 文件路径 | 说明 |
|---------|------|
| `crates/desktop/src/streamdown.css` | Streamdown 样式文件 |
| `crates/desktop/src/components/ui/code-block.tsx` | 旧代码块组件（逻辑迁移到新位置） |

### 修改文件

| 文件路径 | 变更说明 |
|---------|---------|
| `crates/desktop/src/App.tsx` | 移除 streamdown-dev 页面，更新 chat 页面 |
| `crates/desktop/package.json` | 移除 streamdown 依赖，添加 assistant-ui 及相关依赖 |
| `crates/desktop/src/index.css` | 移除 streamdown 相关 @source 指令 |

## 依赖变更

### 移除

```json
"streamdown": "^2.4.0",
"@streamdown/cjk": "^1.0.2",
"@streamdown/code": "^1.1.0",
"@streamdown/math": "^1.0.2",
"@streamdown/mermaid": "^1.0.2"
```

### 新增

> 注意：`shiki` 和 `mermaid` 原本是 streamdown 插件的传递依赖，移除 streamdown 后需要显式安装。

```json
"@assistant-ui/react": "^0.10.0",
"@assistant-ui/react-markdown": "^0.10.0",
"shiki": "^3.0.0",
"mermaid": "^11.0.0",
"remark-gfm": "^4.0.0",
"remark-math": "^6.0.0",
"rehype-katex": "^7.0.0",
"katex": "^0.16.0"
```

## 实现细节

### 1. Mock Runtime

```typescript
// hooks/useMockRuntime.ts
import { useState, useCallback } from "react";
import { useExternalStoreRuntime } from "@assistant-ui/react";

interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: Date;
}

// 预定义的 Mock 响应
const MOCK_RESPONSES: Record<string, string> = {
  default: `这是一个 Mock 响应。

\`\`\`typescript
const greeting: string = "Hello, World!";
console.log(greeting);
\`\`\`

支持 **Markdown** 格式。`,
  code: `这是一个代码示例：

\`\`\`rust
fn main() {
    println!("Hello from Rust!");
}
\`\`\``,
  mermaid: `这是一个 Mermaid 图表：

\`\`\`mermaid
graph TD
    A[开始] --> B{判断}
    B -->|是| C[执行]
    B -->|否| D[结束]
\`\`\``,
  math: `这是一个数学公式：

$$E = mc^2$$

行内公式：$a^2 + b^2 = c^2$`,
};

function getMockResponse(userMessage: string): string {
  const lowerMessage = userMessage.toLowerCase();
  if (lowerMessage.includes("代码") || lowerMessage.includes("code")) {
    return MOCK_RESPONSES.code;
  }
  if (lowerMessage.includes("图表") || lowerMessage.includes("mermaid")) {
    return MOCK_RESPONSES.mermaid;
  }
  if (lowerMessage.includes("数学") || lowerMessage.includes("math")) {
    return MOCK_RESPONSES.math;
  }
  return MOCK_RESPONSES.default;
}

export function useMockRuntime() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isRunning, setIsRunning] = useState(false);

  const sendMessage = useCallback(async (content: string) => {
    const userMessage: ChatMessage = {
      id: crypto.randomUUID(),
      role: "user",
      content,
      createdAt: new Date(),
    };

    setMessages((prev) => [...prev, userMessage]);
    setIsRunning(true);

    // 模拟网络延迟
    await new Promise((resolve) => setTimeout(resolve, 500));

    const aiResponse = getMockResponse(content);

    setMessages((prev) => [
      ...prev,
      {
        id: crypto.randomUUID(),
        role: "assistant",
        content: aiResponse,
        createdAt: new Date(),
      },
    ]);
    setIsRunning(false);
  }, []);

  const clearMessages = useCallback(() => {
    setMessages([]);
  }, []);

  return useExternalStoreRuntime({
    initialMessages: messages,
    adapters: {
      sendMessage,
      clearMessages,
    },
  });
}
```

### 2. 自定义 Markdown 渲染

使用 `@assistant-ui/react-markdown` 作为基础，扩展支持代码高亮、Mermaid 和数学公式：

```typescript
// components/chat/markdown/MarkdownText.tsx
import { memo } from "react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import "katex/dist/katex.min.css";

import { CodeBlock } from "./CodeBlock";
import { MermaidBlock } from "./MermaidBlock";

interface MarkdownTextProps {
  content: string;
}

export const MarkdownText = memo(function MarkdownText({
  content,
}: MarkdownTextProps) {
  return (
    <MarkdownTextPrimitive
      content={content}
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        // 代码块处理
        pre: ({ children, ...props }) => {
          // 检测是否是 mermaid 代码块
          const codeElement = children as React.ReactElement;
          const className = codeElement?.props?.className || "";
          const codeContent = codeElement?.props?.children || "";

          if (className.includes("language-mermaid")) {
            return <MermaidBlock code={String(codeContent)} />;
          }

          return <CodeBlock className={className}>{String(codeContent)}</CodeBlock>;
        },
      }}
    />
  );
});
```

### 3. 代码高亮

迁移现有 `code-block.tsx` 逻辑，保留 Shiki 高亮、暗色模式检测和复制功能：

```typescript
// components/chat/markdown/CodeBlock.tsx
import { useState, useEffect, useRef, useCallback } from "react";
import { codeToHtml } from "shiki";

interface CodeBlockProps {
  className?: string;
  children: string;
}

// 从 className 提取语言
function extractLanguage(className: string): string {
  const match = className.match(/language-(\w+)/);
  return match ? match[1] : "text";
}

// 检测当前主题
function useTheme() {
  const [isDark, setIsDark] = useState(false);
  const observerRef = useRef<MutationObserver | null>(null);

  useEffect(() => {
    const html = document.documentElement;

    // 初始检测
    setIsDark(html.classList.contains("dark"));

    // 监听主题变化
    observerRef.current = new MutationObserver(() => {
      setIsDark(html.classList.contains("dark"));
    });

    observerRef.current.observe(html, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => {
      observerRef.current?.disconnect();
    };
  }, []);

  return isDark;
}

export function CodeBlock({ className, children }: CodeBlockProps) {
  const [html, setHtml] = useState<string>("");
  const [copied, setCopied] = useState(false);
  const isDark = useTheme();
  const lang = extractLanguage(className || "");

  useEffect(() => {
    let cancelled = false;

    async function highlight() {
      try {
        const result = await codeToHtml(children, {
          lang,
          theme: isDark ? "github-dark" : "github-light",
        });
        if (!cancelled) {
          setHtml(result);
        }
      } catch (error) {
        // 回退到纯文本
        if (!cancelled) {
          setHtml(`<pre><code>${children}</code></pre>`);
        }
      }
    }

    highlight();

    return () => {
      cancelled = true;
    };
  }, [children, lang, isDark]);

  const handleCopy = useCallback(async () => {
    await navigator.clipboard.writeText(children);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [children]);

  return (
    <div className="relative group my-4">
      {/* 语言标签 */}
      <span className="absolute top-2 left-3 text-xs text-muted-foreground">
        {lang}
      </span>

      {/* 复制按钮 */}
      <button
        onClick={handleCopy}
        className="absolute top-2 right-2 p-1.5 rounded-md bg-muted/50
                   opacity-0 group-hover:opacity-100 transition-opacity
                   hover:bg-muted"
        aria-label="复制代码"
      >
        {copied ? "✓" : "📋"}
      </button>

      {/* 代码内容 */}
      <div
        className="rounded-lg overflow-x-auto text-sm"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  );
}
```

### 4. Mermaid 图表

使用 `mermaid.render()` API 正确渲染图表：

```typescript
// components/chat/markdown/MermaidBlock.tsx
import { useState, useEffect, useRef } from "react";
import mermaid from "mermaid";

interface MermaidBlockProps {
  code: string;
}

// 生成唯一 ID
let mermaidIdCounter = 0;

export function MermaidBlock({ code }: MermaidBlockProps) {
  const [svg, setSvg] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;

    async function render() {
      try {
        // 初始化 mermaid（只需执行一次）
        mermaid.initialize({
          startOnLoad: false,
          theme: document.documentElement.classList.contains("dark")
            ? "dark"
            : "default",
        });

        const id = `mermaid-${++mermaidIdCounter}`;
        const { svg } = await mermaid.render(id, code);

        if (!cancelled) {
          setSvg(svg);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Mermaid 渲染失败");
          setSvg("");
        }
      }
    }

    render();

    return () => {
      cancelled = true;
    };
  }, [code]);

  if (error) {
    return (
      <div className="my-4 p-4 rounded-lg bg-destructive/10 text-destructive">
        <p className="font-medium">Mermaid 图表渲染失败</p>
        <p className="text-sm mt-1">{error}</p>
        <pre className="mt-2 text-xs bg-muted p-2 rounded overflow-x-auto">
          {code}
        </pre>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="my-4 p-4 rounded-lg bg-muted/50 overflow-x-auto"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
```

### 5. ChatPage 主组件

```typescript
// components/chat/ChatPage.tsx
import { AssistantRuntimeProvider } from "@assistant-ui/react";
import { Thread } from "@assistant-ui/react";
import { useMockRuntime } from "@/hooks/useMockRuntime";
import { MarkdownText } from "./markdown/MarkdownText";

export function ChatPage() {
  const runtime = useMockRuntime();

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <div className="flex flex-col h-full">
        <Thread
          components={{
            MessageContent: ({ message }) => (
              <MarkdownText content={message.content} />
            ),
          }}
        />
      </div>
    </AssistantRuntimeProvider>
  );
}
```

## 风险与缓解

| 风险 | 缓解措施 |
|-----|---------|
| assistant-ui 版本较新，API 可能变化 | 锁定具体版本号 ^0.10.0，升级前测试 |
| Mermaid 客户端渲染性能 | 考虑懒加载或限制图表复杂度 |
| KaTeX 样式冲突 | 使用 CSS scoped 样式隔离，引入 katex.min.css |
| shiki 包体积较大 | 按需加载语言包，使用动态 import |

## 测试计划

### 单元测试 (Vitest)

| 测试项 | 验证内容 |
|-------|---------|
| `useMockRuntime` | 消息发送、状态更新正确 |
| `extractLanguage` | 从 className 正确提取语言 |
| `getMockResponse` | 根据关键词返回对应 Mock 响应 |

### 集成测试

| 测试项 | 验证内容 |
|-------|---------|
| ChatPage 渲染 | 组件树正确挂载，无报错 |
| MarkdownText | GFM、数学公式渲染正确 |
| CodeBlock | Shiki 高亮生效，暗色主题切换 |

### 视觉测试

| 测试项 | 验证内容 |
|-------|---------|
| 亮色/暗色主题 | 颜色对比度、代码高亮主题正确切换 |
| 响应式布局 | 不同屏幕尺寸下布局正常 |

## 时间线

- **Phase 1**: 安装依赖，搭建基础结构
- **Phase 2**: 实现 Mock runtime 和 ChatPage
- **Phase 3**: 迁移 markdown 渲染（代码、Mermaid、数学）
- **Phase 4**: 清理旧代码和依赖

## 参考

- [assistant-ui 文档](https://www.assistant-ui.com/docs)
- [ExternalStoreRuntime API](https://www.assistant-ui.com/docs/runtimes/custom/external-store)
- [Shiki 文档](https://shiki.style/)
- [Mermaid 文档](https://mermaid.js.org/)
- [KaTeX 文档](https://katex.org/)
