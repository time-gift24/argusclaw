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
| `src/components/chat/ChatPage.tsx` | 主聊天页面组件 |
| `src/components/chat/markdown/MarkdownText.tsx` | 自定义 markdown 渲染组件 |
| `src/components/chat/markdown/CodeBlock.tsx` | 代码高亮组件（基于 Shiki） |
| `src/components/chat/markdown/MermaidBlock.tsx` | Mermaid 图表渲染 |
| `src/components/chat/markdown/MathBlock.tsx` | KaTeX 数学公式渲染 |
| `src/hooks/useMockRuntime.ts` | Mock runtime hook |
| `src/lib/chat-types.ts` | 聊天相关类型定义 |

### 删除文件

| 文件路径 | 说明 |
|---------|------|
| `src/streamdown.css` | Streamdown 样式文件 |
| `src/components/ui/code-block.tsx` | 旧的代码块组件 |

### 修改文件

| 文件路径 | 变更说明 |
|---------|---------|
| `src/App.tsx` | 移除 streamdown-dev 页面，更新 chat 页面 |
| `package.json` | 移除 streamdown 依赖，添加 assistant-ui |
| `src/index.css` | 移除 streamdown 相关 @source 指令 |

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

```json
"@assistant-ui/react": "^0.10.x",
"remark-gfm": "^4.x",
"remark-math": "^6.x",
"rehype-katex": "^7.x",
"mermaid": "^11.x"
```

### 保留

```json
"shiki": "^3.x"  // 代码高亮，继续使用
```

## 实现细节

### 1. Mock Runtime

```typescript
// useMockRuntime.ts
import { useExternalStoreRuntime } from "@assistant-ui/react";

interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: Date;
}

export function useMockRuntime() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [isRunning, setIsRunning] = useState(false);

  const sendMessage = useCallback(async (content: string) => {
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content,
      createdAt: new Date(),
    };

    setMessages((prev) => [...prev, userMessage]);
    setIsRunning(true);

    // 模拟 AI 响应
    const aiResponse = await mockAIResponse(content);

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

  return useExternalStoreRuntime({
    messages,
    isRunning,
    sendMessage,
    adapters: {
      // 转换消息格式
    },
  });
}
```

### 2. 自定义 Markdown 渲染

使用 `@assistant-ui/react-markdown` 作为基础，扩展支持：

```typescript
// MarkdownText.tsx
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import { CodeBlock } from "./CodeBlock";
import { MermaidBlock } from "./MermaidBlock";
import { MathBlock } from "./MathBlock";

export function MarkdownText({ content }: { content: string }) {
  return (
    <MarkdownTextPrimitive
      content={content}
      components={{
        pre: CodeBlock,
        // 自定义代码块、数学公式、Mermaid 处理
      }}
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
    />
  );
}
```

### 3. 代码高亮

复用现有 Shiki 配置，迁移到新组件：

```typescript
// CodeBlock.tsx
import { codeToHtml } from "shiki";

export function CodeBlock({ children, className }: CodeBlockProps) {
  const [html, setHtml] = useState("");

  useEffect(() => {
    const lang = extractLanguage(className);
    codeToHtml(children, {
      lang,
      theme: isDark ? "github-dark" : "github-light",
    }).then(setHtml);
  }, [children, className]);

  return (
    <div className="relative group">
      <CopyButton />
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
}
```

### 4. Mermaid 图表

```typescript
// MermaidBlock.tsx
import mermaid from "mermaid";

export function MermaidBlock({ code }: { code: string }) {
  const [svg, setSvg] = useState("");

  useEffect(() => {
    mermaid.run({ nodes: [element] }).then(() => {
      // 渲染完成
    });
  }, [code]);

  return <div ref={elementRef} className="mermaid">{code}</div>;
}
```

### 5. 数学公式

使用 remark-math + rehype-katex：

```typescript
// 在 MarkdownText 中配置
remarkPlugins={[remarkMath]}
rehypePlugins={[rehypeKatex]}
```

## 风险与缓解

| 风险 | 缓解措施 |
|-----|---------|
| assistant-ui 版本较新，API 可能变化 | 锁定具体版本号，升级前测试 |
| Mermaid 客户端渲染性能 | 考虑懒加载或限制图表复杂度 |
| KaTeX 样式冲突 | 使用 CSS scoped 样式隔离 |

## 测试计划

1. **单元测试**：Mock runtime 的消息发送逻辑
2. **集成测试**：Markdown 渲染各组件正确性
3. **视觉测试**：暗色/亮色主题切换

## 时间线

- Phase 1: 安装依赖，搭建基础结构
- Phase 2: 实现 Mock runtime 和 ChatPage
- Phase 3: 迁移 markdown 渲染（代码、Mermaid、数学）
- Phase 4: 清理旧代码和依赖

## 参考

- [assistant-ui 文档](https://www.assistant-ui.com/docs)
- [ExternalStoreRuntime API](https://www.assistant-ui.com/docs/runtimes/custom/external-store)
- [Shiki 文档](https://shiki.style/)
- [Mermaid 文档](https://mermaid.js.org/)
