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
