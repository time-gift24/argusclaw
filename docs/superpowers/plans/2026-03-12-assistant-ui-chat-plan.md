# assistant-ui Chat 页面替换 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 assistant-ui 替换 chat 页面，移除 streamdown 依赖，保留代码高亮、Mermaid 图表、数学公式功能

**Architecture:** 使用 assistant-ui 的 ExternalStoreRuntime 管理 Mock 数据，自定义 MarkdownText 组件集成 Shiki/KaTeX/Mermaid 渲染

**Tech Stack:** React 19, assistant-ui, Shiki, KaTeX, Mermaid, Tailwind CSS v4

---

## File Structure

```
crates/desktop/src/
├── components/
│   ├── chat/
│   │   ├── ChatPage.tsx          # 主聊天页面（新增）
│   │   └── markdown/
│   │       ├── MarkdownText.tsx  # Markdown 渲染器（新增）
│   │       ├── CodeBlock.tsx     # 代码高亮（迁移自 code-block.tsx）
│   │       └── MermaidBlock.tsx  # Mermaid 图表（新增，从 CodeBlock 提取）
│   └── ui/
│       └── code-block.tsx        # 删除
├── hooks/
│   └── useMockRuntime.ts         # Mock runtime（新增）
├── lib/
│   └── chat-types.ts             # 类型定义（新增）
├── App.tsx                       # 修改：移除 streamdown-dev 页面
├── index.css                     # 修改：移除 streamdown @source
└── streamdown.css                # 删除
```

---

## Chunk 1: 依赖管理和文件结构

### Task 1.1: 更新依赖

**Files:**
- Modify: `crates/desktop/package.json`

- [ ] **Step 1: 移除 streamdown 相关依赖**

```bash
cd crates/desktop && pnpm remove streamdown @streamdown/cjk @streamdown/code @streamdown/math @streamdown/mermaid
```

Expected: 依赖成功移除

- [ ] **Step 2: 安装 assistant-ui 及相关依赖**

```bash
cd crates/desktop && pnpm add @assistant-ui/react @assistant-ui/react-markdown shiki mermaid remark-gfm remark-math rehype-katex katex
```

Expected: 依赖成功安装

- [ ] **Step 3: 验证 package.json 更新**

Run: `cat crates/desktop/package.json | grep -E "(assistant-ui|shiki|mermaid|katex|remark)"`

Expected: 看到新增依赖，无 streamdown 相关依赖

- [ ] **Step 4: Commit**

```bash
git add crates/desktop/package.json crates/desktop/pnpm-lock.yaml
git commit -m "chore(deps): replace streamdown with assistant-ui

- Remove streamdown and @streamdown/* packages
- Add @assistant-ui/react and @assistant-ui/react-markdown
- Add shiki, mermaid, remark-gfm, remark-math, rehype-katex, katex

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 1.2: 创建目录结构

**Files:**
- Create: `crates/desktop/src/components/chat/` (directory)
- Create: `crates/desktop/src/components/chat/markdown/` (directory)
- Create: `crates/desktop/src/lib/chat-types.ts`

- [ ] **Step 1: 创建聊天组件目录**

```bash
mkdir -p crates/desktop/src/components/chat/markdown
```

Expected: 目录创建成功

- [ ] **Step 2: 创建类型定义文件**

```typescript
// crates/desktop/src/lib/chat-types.ts

/**
 * 聊天消息类型定义
 */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: Date;
}

/**
 * 消息角色类型
 */
export type MessageRole = "user" | "assistant";

/**
 * Mock 响应映射类型
 */
export type MockResponseKey = "default" | "code" | "mermaid" | "math";
```

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/src/components/chat/ crates/desktop/src/lib/chat-types.ts
git commit -m "feat(chat): add chat component directory and types

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 2: Mock Runtime 实现

### Task 2.1: 实现 Mock Runtime Hook

**Files:**
- Create: `crates/desktop/src/hooks/useMockRuntime.ts`

- [ ] **Step 1: 创建 useMockRuntime hook**

```typescript
// crates/desktop/src/hooks/useMockRuntime.ts

import { useState, useCallback } from "react";
import { useExternalStoreRuntime } from "@assistant-ui/react";
import type { ChatMessage, MockResponseKey } from "@/lib/chat-types";

/**
 * 预定义的 Mock 响应
 */
const MOCK_RESPONSES: Record<MockResponseKey, string> = {
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
 * Mock Runtime Hook
 * 提供 Mock 数据供 assistant-ui 使用
 */
export function useMockRuntime() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isRunning, setIsRunning] = useState(false);

  const sendMessage = useCallback(async (content: string) => {
    // 创建用户消息
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

    // 获取 Mock 响应
    const aiResponse = getMockResponse(content);

    // 创建 AI 响应消息
    const assistantMessage: ChatMessage = {
      id: crypto.randomUUID(),
      role: "assistant",
      content: aiResponse,
      createdAt: new Date(),
    };

    setMessages((prev) => [...prev, assistantMessage]);
    setIsRunning(false);
  }, []);

  const clear = useCallback(() => {
    setMessages([]);
  }, []);

  return useExternalStoreRuntime({
    initialMessages: messages,
    adapters: {
      sendMessage,
      clear,
    },
  });
}
```

- [ ] **Step 2: 验证文件创建**

Run: `ls -la crates/desktop/src/hooks/`

Expected: 看到 `useMockRuntime.ts` 文件

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/src/hooks/useMockRuntime.ts
git commit -m "feat(chat): add mock runtime hook for assistant-ui

- Implement useMockRuntime with ExternalStoreRuntime
- Add predefined mock responses for code, mermaid, math
- Support keyword-based response selection

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 3: Markdown 渲染组件

### Task 3.1: 创建 CodeBlock 组件（迁移自现有实现）

**Files:**
- Create: `crates/desktop/src/components/chat/markdown/CodeBlock.tsx`

- [ ] **Step 1: 创建 CodeBlock 组件**

```typescript
// crates/desktop/src/components/chat/markdown/CodeBlock.tsx

import { useState, useEffect, useRef, useCallback, useMemo, type ReactNode } from "react";
import { CopyIcon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { codeToHtml, bundledThemes, type ThemeRegistration } from "shiki";

interface CodeBlockProps {
  className?: string;
  children?: ReactNode;
  language?: string;
  code?: string;
  isIncomplete?: boolean; // 流式输出时使用
}

// 背景色常量
const LIGHT_BG = "#f1f5f9";
const DARK_BG = "#1e1e1e";

// 主题缓存
let customLightTheme: ThemeRegistration | null = null;
let customDarkTheme: ThemeRegistration | null = null;

async function loadCustomThemes() {
  if (!customLightTheme) {
    const lightTheme = await bundledThemes["github-light"]().then((m) => m.default);
    customLightTheme = {
      ...lightTheme,
      bg: LIGHT_BG,
      colors: {
        ...lightTheme.colors,
        "editor.background": LIGHT_BG,
      },
    };
  }
  if (!customDarkTheme) {
    const darkTheme = await bundledThemes["github-dark"]().then((m) => m.default);
    customDarkTheme = {
      ...darkTheme,
      bg: DARK_BG,
      colors: {
        ...darkTheme.colors,
        "editor.background": DARK_BG,
      },
    };
  }
  return { light: customLightTheme!, dark: customDarkTheme! };
}

// 预加载主题
loadCustomThemes();

// 语言别名映射
const LANG_MAP: Record<string, string> = {
  ts: "typescript",
  js: "javascript",
  py: "python",
  rb: "ruby",
  sh: "bash",
  shell: "bash",
  yml: "yaml",
};

/**
 * 从 className 提取语言
 */
function extractLanguage(className: string): string {
  const match = className.match(/language-(\w+)/);
  return match ? match[1].toLowerCase() : "text";
}

/**
 * 提取子节点文本
 */
function extractTextFromChildren(children: ReactNode): string {
  if (typeof children === "string") return children;
  if (typeof children === "number") return String(children);
  if (!children) return "";

  if (Array.isArray(children)) {
    return children.map(extractTextFromChildren).join("");
  }

  if (typeof children === "object" && "props" in children) {
    return extractTextFromChildren(
      (children as ReactNode & { props?: { children?: ReactNode } }).props?.children
    );
  }

  return "";
}

/**
 * 检测当前主题
 */
function useTheme() {
  const [isDark, setIsDark] = useState(false);

  useEffect(() => {
    const checkTheme = () => {
      setIsDark(document.documentElement.classList.contains("dark"));
    };

    checkTheme();

    const observer = new MutationObserver(checkTheme);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  return isDark;
}

/**
 * 代码块组件
 * - Shiki 语法高亮
 * - 明暗主题支持
 * - 复制功能
 */
export function CodeBlock({ className, children, language: langProp, code: rawCode, isIncomplete }: CodeBlockProps) {
  const [highlightedHtml, setHighlightedHtml] = useState("");
  const [copied, setCopied] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const isDark = useTheme();

  // 提取语言
  const language = langProp || extractLanguage(className || "");

  // 提取代码文本
  const codeText = useMemo(() => {
    if (rawCode) return rawCode;
    return extractTextFromChildren(children);
  }, [rawCode, children]);

  // 背景色
  const codeBg = isDark ? DARK_BG : LIGHT_BG;

  // 语法高亮
  useEffect(() => {
    if (!codeText) {
      setHighlightedHtml("");
      setIsLoading(false);
      return;
    }

    const highlight = async () => {
      const mappedLang = LANG_MAP[language] || language || "text";

      try {
        const themes = await loadCustomThemes();
        const html = await codeToHtml(codeText, {
          lang: mappedLang,
          themes: {
            light: themes.light,
            dark: themes.dark,
          },
          defaultColor: isDark ? "dark" : "light",
        });
        setHighlightedHtml(html);
      } catch (err) {
        console.warn("Shiki highlighting failed:", err);
        // 回退到纯文本
        setHighlightedHtml(`<pre style="margin:0"><code>${escapeHtml(codeText)}</code></pre>`);
      } finally {
        setIsLoading(false);
      }
    };

    highlight();
  }, [codeText, language, isDark]);

  // 复制功能
  const handleCopy = useCallback(async () => {
    if (!codeText) return;

    try {
      await navigator.clipboard.writeText(codeText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  }, [codeText]);

  // 流式输出或加载中显示骨架屏
  if (isIncomplete || isLoading) {
    return (
      <div className="my-3 rounded-lg overflow-hidden">
        <div
          className="flex items-center justify-between px-2 py-1 border-b"
          style={{ backgroundColor: codeBg, borderColor: "var(--border, #e2e8f0)" }}
        >
          <span
            className="text-[12px] font-medium"
            style={{ color: "var(--muted-foreground, #64748b)" }}
          >
            {language || "code"}
          </span>
        </div>
        <div className="p-3" style={{ backgroundColor: codeBg }}>
          <div className="animate-pulse h-4 w-3/4 rounded" style={{ backgroundColor: "var(--muted, #e2e8f0)" }} />
        </div>
      </div>
    );
  }

  if (!codeText) {
    return null;
  }

  return (
    <div className="my-3 rounded-lg overflow-hidden">
      {/* Header */}
      <div
        className="flex items-center justify-between px-2 py-1 border-b"
        style={{ backgroundColor: codeBg, borderColor: "var(--border, #e2e8f0)" }}
      >
        <span
          className="text-[12px] font-medium"
          style={{ color: "var(--muted-foreground, #64748b)" }}
        >
          {language || "code"}
        </span>
        <button
          onClick={handleCopy}
          className="p-1 rounded hover:opacity-80 transition-opacity"
          style={{ color: "var(--muted-foreground, #64748b)" }}
          title={copied ? "已复制!" : "复制代码"}
        >
          <HugeiconsIcon icon={CopyIcon} className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Code content */}
      <div
        className="p-3 overflow-x-auto"
        style={{ backgroundColor: codeBg, fontSize: "var(--sd-code-font-size, 13px)" }}
      >
        <code
          dangerouslySetInnerHTML={{ __html: highlightedHtml || escapeHtml(codeText) }}
        />
      </div>
    </div>
  );
}

/**
 * HTML 转义
 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/src/components/chat/markdown/CodeBlock.tsx
git commit -m "feat(chat): add CodeBlock component with Shiki highlighting

- Migrate from existing code-block.tsx with improvements
- Support light/dark theme with MutationObserver
- Add copy functionality
- Add language alias mapping

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3.2: 创建 MermaidBlock 组件

**Files:**
- Create: `crates/desktop/src/components/chat/markdown/MermaidBlock.tsx`

- [ ] **Step 1: 创建 MermaidBlock 组件**

```typescript
// crates/desktop/src/components/chat/markdown/MermaidBlock.tsx

import { useState, useEffect, useRef } from "react";
import mermaid from "mermaid";

interface MermaidBlockProps {
  code: string;
}

// 全局 ID 计数器
let mermaidIdCounter = 0;

// 初始化 mermaid（模块级别，只执行一次）
mermaid.initialize({
  startOnLoad: false,
  theme: "default",
  securityLevel: "loose",
});

/**
 * Mermaid 图表渲染组件
 */
export function MermaidBlock({ code }: MermaidBlockProps) {
  const [svg, setSvg] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!code) {
      setSvg("");
      setError(null);
      return;
    }

    let cancelled = false;

    const renderDiagram = async () => {
      try {
        // 根据当前主题更新 mermaid 配置
        const isDark = document.documentElement.classList.contains("dark");
        mermaid.initialize({
          startOnLoad: false,
          theme: isDark ? "dark" : "default",
          securityLevel: "loose",
        });

        // 生成唯一 ID
        const id = `mermaid-${++mermaidIdCounter}`;

        // 渲染图表
        const { svg: renderedSvg } = await mermaid.render(id, code);

        if (!cancelled) {
          setSvg(renderedSvg);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          const errorMessage = err instanceof Error ? err.message : "Mermaid 渲染失败";
          setError(errorMessage);
          setSvg("");
        }
      }
    };

    renderDiagram();

    return () => {
      cancelled = true;
    };
  }, [code]);

  if (error) {
    return (
      <div className="my-4 p-4 rounded-lg bg-destructive/10 text-destructive border border-destructive/20">
        <p className="font-medium text-sm">Mermaid 图表渲染失败</p>
        <p className="text-xs mt-1 opacity-80">{error}</p>
        <pre className="mt-2 text-xs bg-muted/50 p-2 rounded overflow-x-auto">
          {code}
        </pre>
      </div>
    );
  }

  if (!svg) {
    return (
      <div className="my-4 p-4 rounded-lg bg-muted/50 animate-pulse">
        <div className="h-20 bg-muted rounded" />
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="my-4 p-4 rounded-lg bg-muted/30 overflow-x-auto flex justify-center"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/src/components/chat/markdown/MermaidBlock.tsx
git commit -m "feat(chat): add MermaidBlock component for diagram rendering

- Use mermaid.render() API
- Support light/dark theme
- Add error handling with fallback UI
- Add loading state

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3.3: 创建 MarkdownText 组件

**Files:**
- Create: `crates/desktop/src/components/chat/markdown/MarkdownText.tsx`

- [ ] **Step 1: 创建 MarkdownText 组件**

```typescript
// crates/desktop/src/components/chat/markdown/MarkdownText.tsx

import { memo, type ReactElement } from "react";
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

/**
 * 自定义 Markdown 渲染组件
 * - 支持 GFM (GitHub Flavored Markdown)
 * - 支持数学公式 (KaTeX)
 * - 支持代码高亮 (Shiki)
 * - 支持 Mermaid 图表
 */
export const MarkdownText = memo(function MarkdownText({ content }: MarkdownTextProps) {
  return (
    <MarkdownTextPrimitive
      content={content}
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        // 自定义代码块处理
        pre: ({ children, ...props }) => {
          const codeElement = children as ReactElement<{
            className?: string;
            children?: string;
          }>;

          const className = codeElement?.props?.className || "";
          const codeContent = codeElement?.props?.children || "";

          // 检测是否是 mermaid 代码块
          if (className.includes("language-mermaid")) {
            return <MermaidBlock code={String(codeContent)} />;
          }

          // 普通代码块
          return (
            <CodeBlock className={className}>
              {String(codeContent)}
            </CodeBlock>
          );
        },
      }}
    />
  );
});
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/src/components/chat/markdown/MarkdownText.tsx
git commit -m "feat(chat): add MarkdownText component with full markdown support

- Integrate remark-gfm, remark-math, rehype-katex
- Custom code block handling with Shiki and Mermaid
- Export memoized component for performance

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3.4: 创建 index 导出文件

**Files:**
- Create: `crates/desktop/src/components/chat/markdown/index.ts`

- [ ] **Step 1: 创建 index 文件**

```typescript
// crates/desktop/src/components/chat/markdown/index.ts

export { MarkdownText } from "./MarkdownText";
export { CodeBlock } from "./CodeBlock";
export { MermaidBlock } from "./MermaidBlock";
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/src/components/chat/markdown/index.ts
git commit -m "feat(chat): add markdown components index export

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 4: ChatPage 和 App 集成

### Task 4.1: 创建 ChatPage 组件

**Files:**
- Create: `crates/desktop/src/components/chat/ChatPage.tsx`

- [ ] **Step 1: 创建 ChatPage 组件**

```typescript
// crates/desktop/src/components/chat/ChatPage.tsx

import { AssistantRuntimeProvider, Thread } from "@assistant-ui/react";
import { useMockRuntime } from "@/hooks/useMockRuntime";
import { MarkdownText } from "./markdown";

/**
 * 聊天页面组件
 * 使用 assistant-ui 的 Thread 组件和自定义 Markdown 渲染
 */
export function ChatPage() {
  const runtime = useMockRuntime();

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <div className="flex flex-col h-full">
        <Thread />
      </div>
    </AssistantRuntimeProvider>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/src/components/chat/ChatPage.tsx
git commit -m "feat(chat): add ChatPage component with assistant-ui Thread

- Use ExternalStoreRuntime via useMockRuntime
- Wrap with AssistantRuntimeProvider

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4.2: 更新 App.tsx

**Files:**
- Modify: `crates/desktop/src/App.tsx`

- [ ] **Step 1: 移除 streamdown 导入，添加 ChatPage 导入**

找到文件开头的导入部分：

```typescript
// 删除这些行:
import "./streamdown.css";
import { Streamdown } from "streamdown";
import { math } from "@streamdown/math";
import { cjk } from "@streamdown/cjk";
import { CodeBlock } from "@/components/ui/code-block";
```

替换为：

```typescript
import { ChatPage } from "@/components/chat/ChatPage";
```

- [ ] **Step 2: 更新页面类型**

找到 `type Page` 定义，修改为：

```typescript
// 页面类型
type Page = "chat";
```

- [ ] **Step 3: 简化 App 组件，移除 streamdown-dev 页面**

将 `App` 函数替换为简化版本：

```typescript
function App() {
  const [currentPage, setCurrentPage] = useState<Page>("chat");

  return (
    <TooltipProvider>
      <SidebarProvider defaultOpen={true}>
        <Sidebar variant="floating" collapsible="offcanvas">
          <SidebarHeader className="py-4">
            <div className="flex items-center justify-center">
              <span className="text-lg font-semibold">ArgusClaw</span>
            </div>
          </SidebarHeader>
          <SidebarContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  isActive={currentPage === "chat"}
                  onClick={() => setCurrentPage("chat")}
                >
                  <HugeiconsIcon icon={ChatIcon} />
                  <span>聊天</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarContent>
        </Sidebar>
        <SidebarInset>
          <header className="flex h-14 items-center gap-2 border-b px-4">
            <SidebarTrigger />
            <span className="text-sm font-medium">
              聊天
            </span>
          </header>
          <ChatPage />
        </SidebarInset>
      </SidebarProvider>
    </TooltipProvider>
  );
}
```

- [ ] **Step 4: 删除旧的组件定义**

删除以下组件和常量：
- `SAMPLE_MARKDOWN`
- `StyleConfig` interface
- `DEFAULT_STYLES`
- `ChatView` 函数
- `StyleControls` 函数
- `StreamdownDevPage` 函数
- 旧的 `ChatPage` 函数（占位符版本）

- [ ] **Step 5: 清理未使用的导入**

确保只保留实际使用的导入：
- `React` 可以移除（React 19 不需要显式导入）
- 移除 `CodeIcon`, `AiMagicIcon`（如果不再使用）
- 保留 `ChatIcon`

最终的 App.tsx 应该类似于：

```typescript
import {
  Sidebar,
  SidebarContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { HugeiconsIcon } from "@hugeicons/react";
import { ChatIcon } from "@hugeicons/core-free-icons";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useState } from "react";
import { ChatPage } from "@/components/chat/ChatPage";

// 页面类型
type Page = "chat";

function App() {
  const [currentPage, setCurrentPage] = useState<Page>("chat");

  return (
    <TooltipProvider>
      <SidebarProvider defaultOpen={true}>
        <Sidebar variant="floating" collapsible="offcanvas">
          <SidebarHeader className="py-4">
            <div className="flex items-center justify-center">
              <span className="text-lg font-semibold">ArgusClaw</span>
            </div>
          </SidebarHeader>
          <SidebarContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  isActive={currentPage === "chat"}
                  onClick={() => setCurrentPage("chat")}
                >
                  <HugeiconsIcon icon={ChatIcon} />
                  <span>聊天</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarContent>
        </Sidebar>
        <SidebarInset>
          <header className="flex h-14 items-center gap-2 border-b px-4">
            <SidebarTrigger />
            <span className="text-sm font-medium">聊天</span>
          </header>
          <ChatPage />
        </SidebarInset>
      </SidebarProvider>
    </TooltipProvider>
  );
}

export default App;
```

- [ ] **Step 6: Commit**

```bash
git add crates/desktop/src/App.tsx
git commit -m "feat(chat): integrate ChatPage into App, remove streamdown-dev

- Remove all streamdown related code
- Remove StyleConfig, StyleControls, ChatView, StreamdownDevPage
- Simplify App to only use ChatPage

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4.3: 更新 index.css

**Files:**
- Modify: `crates/desktop/src/index.css`

- [ ] **Step 1: 移除 streamdown @source 指令**

删除以下行：

```css
/* Streamdown */
@source "../node_modules/streamdown/dist/*.js";
@source "../node_modules/@streamdown/math/dist/*.js";
@source "../node_modules/@streamdown/cjk/dist/*.js";
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/src/index.css
git commit -m "style: remove streamdown @source directives from index.css

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 5: 清理旧文件

### Task 5.1: 删除旧文件

**Files:**
- Delete: `crates/desktop/src/streamdown.css`
- Delete: `crates/desktop/src/components/ui/code-block.tsx`

- [ ] **Step 1: 删除 streamdown.css**

```bash
rm crates/desktop/src/streamdown.css
```

Expected: 文件删除成功

- [ ] **Step 2: 删除旧的 code-block.tsx**

```bash
rm crates/desktop/src/components/ui/code-block.tsx
```

Expected: 文件删除成功

- [ ] **Step 3: 验证文件已删除**

Run: `ls crates/desktop/src/components/ui/`

Expected: 不包含 `code-block.tsx`

Run: `ls crates/desktop/src/`

Expected: 不包含 `streamdown.css`

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: remove obsolete streamdown.css and code-block.tsx

- Delete streamdown.css (replaced by assistant-ui styling)
- Delete old code-block.tsx (migrated to chat/markdown/CodeBlock.tsx)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 6: 验证和测试

### Task 6.1: 验证构建

- [ ] **Step 1: 运行 TypeScript 类型检查**

Run: `cd crates/desktop && pnpm exec tsc --noEmit`

Expected: 无类型错误

- [ ] **Step 2: 运行开发服务器验证**

Run: `cd crates/desktop && pnpm dev`

Expected: 开发服务器启动成功，无编译错误

- [ ] **Step 3: 手动测试聊天功能**

1. 在浏览器中打开应用
2. 在输入框中输入消息
3. 验证以下场景：

| 输入关键词 | 预期响应 |
|-----------|---------|
| "hello" 或普通文本 | default 响应（含 TypeScript 代码） |
| "代码" 或 "code" | code 响应（含 Rust 和 Python 代码） |
| "图表" 或 "mermaid" 或 "流程" | mermaid 响应（含流程图） |
| "数学" 或 "math" 或 "公式" | math 响应（含 KaTeX 公式） |

4. 验证各功能正常：
   - 用户消息显示正常
   - Mock 响应返回正确
   - 代码高亮正常
   - Mermaid 图表渲染正常
   - 数学公式渲染正常

- [ ] **Step 4: 测试主题切换**

1. 切换明暗主题
2. 验证代码高亮主题切换正确
3. 验证 Mermaid 图表主题切换正确

---

### Task 6.2: 更新文档

**Files:**
- Modify: `crates/desktop/CLAUDE.md`

- [ ] **Step 1: 更新技术栈描述**

将 `crates/desktop/CLAUDE.md` 中的 Streamdown 相关描述替换为 assistant-ui：

```markdown
## 技术栈

- **UI 框架**: React 19
- **语言**: TypeScript
- **构建工具**: Vite
- **样式**: Tailwind CSS v4
- **聊天 UI**: assistant-ui + 自定义 Markdown 渲染
- **Markdown 渲染**: @assistant-ui/react-markdown + Shiki + KaTeX + Mermaid
- **UI 组件**: shadcn (基于 class-variance-authority, clsx, tailwind-merge)
- **桌面框架**: Tauri
- **图标**: @hugeicons/react + @hugeicons/core-free-icons
```

- [ ] **Step 2: 更新开发命令部分**

移除 Streamdown 配置相关内容，添加 assistant-ui 相关说明：

```markdown
## assistant-ui 配置

### 基本用法

```tsx
import { AssistantRuntimeProvider, Thread } from "@assistant-ui/react";
import { useMockRuntime } from "@/hooks/useMockRuntime";

export function ChatPage() {
  const runtime = useMockRuntime();
  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <Thread />
    </AssistantRuntimeProvider>
  );
}
```

### 自定义 Markdown 渲染

聊天消息使用自定义 Markdown 渲染器，位于 `src/components/chat/markdown/`：
- `MarkdownText.tsx` - 主渲染器，集成 GFM、数学公式
- `CodeBlock.tsx` - Shiki 代码高亮
- `MermaidBlock.tsx` - Mermaid 图表渲染
```

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/CLAUDE.md
git commit -m "docs: update CLAUDE.md for assistant-ui migration

- Replace Streamdown references with assistant-ui
- Add assistant-ui usage examples
- Document custom Markdown rendering components

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 6.3: 最终提交

- [ ] **Step 1: 检查 git 状态**

Run: `git status`

Expected: 工作目录干净，无未提交文件

- [ ] **Step 2: 查看提交历史**

Run: `git log --oneline -10`

Expected: 看到一系列相关提交

- [ ] **Step 3: 运行 prek 检查**

Run: `prek`

Expected: 所有检查通过

---

## Notes

### assistant-ui 版本说明

当前使用 `@assistant-ui/react@^0.10.0`，该版本提供了稳定的 ExternalStoreRuntime API。如果遇到 API 变化，请参考：
- [assistant-ui 文档](https://www.assistant-ui.com/docs)
- [ExternalStoreRuntime API](https://www.assistant-ui.com/docs/runtimes/custom/external-store)

### 后续工作

本次实现使用 Mock 数据，后续需要：
1. 实现 Tauri 命令与 claw crate 的 LLM manager 交互
2. 实现对话持久化
3. 实现多会话管理
4. 考虑流式输出支持
