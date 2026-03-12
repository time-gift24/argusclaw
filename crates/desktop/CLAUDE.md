# ArgusClaw Desktop 开发指南

## 技术栈

- **UI 框架**: React 19
- **语言**: TypeScript
- **构建工具**: Vite
- **样式**: Tailwind CSS v4
- **聊天 UI**: assistant-ui + 自定义 Markdown 渲染
- **Markdown 渲染**: react-markdown + Shiki + KaTeX + Mermaid
- **UI 组件**: shadcn (基于 class-variance-authority, clsx, tailwind-merge)
- **桌面框架**: Tauri
- **图标**: @hugeicons/react + @hugeicons/core-free-icons

## 开发命令

```bash
pnpm dev          # 开发模式
pnpm tauri dev    # Tauri 开发模式
pnpm build        # 生产构建
pnpm tauri build  # Tauri 生产构建
```

## assistant-ui 配置

### 基本用法

```tsx
import { AssistantRuntimeProvider } from "@assistant-ui/react";
import { Thread } from "@assistant-ui/react-ui";
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

## 性能优化

遵循 vercel-react-best-practices 规范（参考根目录 `.claude/skills/vercel-react-best-practices`）：

- 避免不必要的重渲染，使用 `React.memo` 优化组件
- 使用 `startTransition` 处理非紧急更新
- 流式内容使用 `Promise.all()` 并行加载
- 第三方库按需加载，使用动态 import
- 代码分割，优先加载关键内容

## 项目结构

```text
crates/desktop/
├── src/
│   ├── main.tsx              # 入口
│   ├── App.tsx               # 根组件
│   ├── index.css             # 全局样式 (含 Tailwind)
│   ├── vite-env.d.ts         # Vite 类型
│   ├── hooks/
│   │   └── useMockRuntime.ts # Mock runtime for assistant-ui
│   └── components/
│       └── chat/
│           ├── ChatPage.tsx      # 聊天页面
│           └── markdown/
│               ├── index.ts      # 导出
│               ├── MarkdownText.tsx  # Markdown 渲染器
│               ├── CodeBlock.tsx     # Shiki 代码高亮
│               └── MermaidBlock.tsx  # Mermaid 图表
├── src-tauri/          # Rust 后端
├── components.json     # shadcn 配置
├── vite.config.ts      # Vite 配置
└── package.json
```
