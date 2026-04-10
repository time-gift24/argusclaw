# ArgusWing Desktop

> 特性：React 19 + Vite 8 + Tauri 2 桌面前端，承载设置、聊天与 thread monitor 界面。

## 目录结构

- `app/`：route-level 页面与布局
- `components/assistant-ui`、`chat`、`settings`、`thread-monitor`、`ui`：界面组件
- `lib/`：runtime、store、Tauri 绑定与共享类型
- `hooks/`：前端状态钩子
- `tests/`：UI 回归与绑定测试
- `src-tauri/`：Rust bridge，见其本地 `CLAUDE.md`

## UI 约束

- 无 SSR，全部按客户端渲染思路设计
- 用户可见文案默认中文
- 保持紧凑桌面密度，常见导航 / 表单 / 按钮文本以 `text-sm` 为主
- `settings`、`chat`、`thread-monitor` 页面延续现有 navbar + breadcrumb + `max-w-7xl` 内容宽度

## 开发命令

```bash
pnpm dev
pnpm tauri dev
pnpm build
pnpm tauri build
```

## 修改守则

- 共享状态优先放在 `lib/*` 或 `hooks/*`，不要在页面组件里偷偷造全局单例
- Tauri contract 变更时，要同步检查 `src-tauri`、`lib/tauri.ts` 与相关测试
- `dist/`、`node_modules/` 不是事实来源，不在里面做手改
