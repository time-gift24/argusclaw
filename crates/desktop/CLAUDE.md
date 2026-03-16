# ArgusWing Desktop 开发指南

## 技术栈

- **UI 框架**: React 19
- **语言**: TypeScript
- **构建工具**: Next.js (仅作为构建工具，无 SSR)
- **样式**: Tailwind CSS v4
- **聊天 UI**: assistant-ui + 自定义 Markdown 渲染
- **Markdown 渲染**: react-markdown + Shiki + KaTeX + Mermaid
- **UI 组件**: shadcn (基于 class-variance-authority, clsx, tailwind-merge)
- **桌面框架**: Tauri
- **图标**: @hugeicons/react + @hugeicons/core-free-icons

## 重要说明

- **无 SSR**: 这是 Tauri 桌面应用，不涉及服务端渲染 (SSR)。所有组件都在客户端渲染，不需要处理 SSR 相关的水合问题。

## 视觉与文案约束

- Desktop 导航、认证弹窗、下拉菜单等高频操作区，**可见文字默认控制在 14px - 18px**。
- 不要为了强调少量操作把认证入口做得过大；登录相关图标优先控制在 `16px` 左右，保持紧凑桌面密度。
- 用户可见的新文案默认使用中文，除非该页面已有明确的英文信息架构约束。
- 登录后头像入口只保留退出登录能力；点击头像直接弹出确认退出登录弹窗，不展示无实际能力的资料/设置类菜单项。

## 开发命令

```bash
pnpm dev          # 开发模式
pnpm tauri dev    # Tauri 开发模式
pnpm build        # 生产构建
pnpm tauri build  # Tauri 生产构建
```

## assistant-ui 配置
