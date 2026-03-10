# ArgusClaw Desktop 开发指南

## 技术栈

- **UI 框架**: React 19
- **语言**: TypeScript
- **构建工具**: Vite
- **样式**: Tailwind CSS v4
- **Markdown 渲染**: Streamdown + 插件 (code, mermaid, math, cjk)
- **UI 组件**: shadcn (基于 class-variance-authority, clsx, tailwind-merge)
- **桌面框架**: Tauri
- **图标**: @hugeicons/react + @hugeicons/core-free-icons

## 开发命令

```bash
pnpm dev          # 开发模式 (Vite 端口 5173)
pnpm tauri dev    # Tauri 开发模式
pnpm build        # 生产构建
pnpm tauri build  # Tauri 生产构建
```

## Streamdown 配置

### Tailwind CSS v4

在 `src/index.css` 中添加 `@source` 指令：

```css
@source "../node_modules/streamdown/dist/*.js";
@source "../node_modules/@streamdown/code/dist/*.js";
@source "../node_modules/@streamdown/mermaid/dist/*.js";
@source "../node_modules/@streamdown/math/dist/*.js";
@source "../node_modules/@streamdown/cjk/dist/*.js";
```

### 基本用法

```tsx
import { Streamdown } from 'streamdown';
import { code } from '@streamdown/code';
import { mermaid } from '@streamdown/mermaid';
import { math } from '@streamdown/math';
import { cjk } from '@streamdown/cjk';

<Streamdown plugins={{ code, mermaid, math, cjk }}>
  {markdownContent}
</Streamdown>
```

### 静态模式 (用于博客/文档)

```tsx
<Streamdown mode="static" plugins={{ code }}>
  {content}
</Streamdown>
```

### 关键配置

- `mode`: `"streaming"` | `"static"` — 渲染模式
- `plugins`: 插件对象 — 功能扩展
- `caret`: `"block" | "circle"` — 光标样式
- `isAnimating`: 配合流式输出使用
- `linkSafety`: 链接安全确认 (默认启用)

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
│   ├── main.tsx        # 入口
│   ├── App.tsx         # 根组件
│   ├── index.css       # 全局样式 (含 Tailwind)
│   └── vite-env.d.ts  # Vite 类型
├── src-tauri/          # Rust 后端
├── components.json     # shadcn 配置
├── vite.config.ts      # Vite 配置
└── package.json
```
