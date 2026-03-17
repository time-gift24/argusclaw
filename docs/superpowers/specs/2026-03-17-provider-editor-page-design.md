# Provider 编辑页面设计

**日期**: 2026-03-17
**状态**: 待实现

## 概述

将 Provider 的编辑和添加功能从弹窗改为独立页面，与 Agent 编辑页面风格保持一致。左侧为基础配置项，右侧为模型可达性测试面板。

## 路由结构

| 路由 | 用途 |
|------|------|
| `/settings/providers/new` | 新建 Provider |
| `/settings/providers/[id]` | 编辑指定 Provider |

## 页面布局

采用左右两栏布局，与现有 Agent 编辑页面风格一致。

### 头部区域

- 返回按钮（左箭头）
- 页面标题：`新建 Provider` 或 `编辑 Provider`
- 保存按钮
- 面包屑：`设置 / LLM 提供者 / 新建（或名称）`

### 左侧：基础配置

与现有 `ProviderFormDialog` 字段完全一致：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| ID | text | 是 | 唯一标识，编辑时禁用 |
| 名称 | text | 是 | 显示名称 |
| Base URL | text | 是 | API 基础地址 |
| API Key | password | 是 | 密钥输入 |
| 模型列表 | badges | - | 可添加/删除模型标签 |
| 设为默认 | checkbox | 否 | 是否为默认 Provider |

**模型列表交互**：
- 输入框 + 添加按钮
- 点击标签设为默认模型（显示"默认"小标签）
- 点击 × 删除模型

### 右侧：模型可达性测试

#### 交互模式（混合）

- **自动测试**：添加模型后自动触发测试
- **手动测试**：提供"全部测试"按钮
- **单模型测试**：点击单个模型行可重新测试

#### 列表视图

每个模型一行，包含：

| 元素 | 说明 |
|------|------|
| 状态图标 | ✓ 成功(绿)、✗ 失败(红)、◐ 测试中(蓝) |
| 模型名称 | 等宽字体显示 |
| 默认标签 | 默认模型显示蓝色标签 |
| 延迟时间 | 成功时显示，如 `324ms` |
| 错误信息 | 失败时显示简短错误，如 `连接超时` |

#### 状态样式

| 状态 | 行背景 | 图标颜色 |
|------|--------|----------|
| 成功 | 无 | #10b981 (green) |
| 失败 | #fef2f2 (red-50) | #ef4444 (red) |
| 测试中 | #eff6ff (blue-50) | #3b82f6 (blue) |

#### 错误详情

失败项下方展示可展开的错误详情卡片：
- 红色边框背景
- 模型名称 + 错误类型
- 完整错误消息（等宽字体）

#### 测试汇总

底部显示汇总信息：
- `测试结果: 2/3 通过`
- `平均延迟: 305ms`（仅成功时）

## 组件结构

```
app/settings/providers/
├── new/page.tsx          # 新建页面
└── [id]/page.tsx         # 编辑页面

components/settings/
├── provider-editor.tsx   # 主编辑组件（新建）
├── provider-test-panel.tsx # 右侧测试面板组件
└── provider-model-list.tsx # 模型列表子组件
```

## 现有组件处理

| 组件 | 处理方式 |
|------|----------|
| `ProviderFormDialog` | 保留，用于列表页快速添加（可选）或删除 |
| `ProviderTestDialog` | 删除，功能合并到新页面 |
| `ProviderCard` | 保留，编辑按钮跳转到独立页面 |

## 技术要点

1. **复用现有 API**：`providers.list()`, `providers.get()`, `providers.upsert()`, `providers.testConnection()`, `providers.testInput()`

2. **状态管理**：使用 React 状态管理表单数据和测试结果，无需引入额外状态库

3. **并发测试**：支持同时测试多个模型，使用 Promise.all 或逐个测试

4. **保存逻辑**：不强制要求所有模型测试通过才能保存

## 迁移计划

1. 创建 `ProviderEditor` 组件
2. 创建 `ProviderTestPanel` 组件
3. 添加路由页面
4. 修改 `ProviderCard` 编辑按钮行为
5. （可选）保留或移除 `ProviderFormDialog`
