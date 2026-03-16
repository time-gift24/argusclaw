# Provider 和模型配置 UI 重构实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 Provider 配置对话框重构为分步形式，实现实时保存 Provider 和模型；添加 Provider 图标自动匹配；在测试连接时使用下拉选择模型。

**Architecture:** 前端 React/TypeScript 改动，将 provider-form-dialog.tsx 从单步表单改为分步对话框；创建图标映射函数；将模型输入改为下拉选择。

**Tech Stack:** Next.js (React), TypeScript, Tailwind CSS, shadcn/ui, @hugeicons/react

---

## Task 1: 创建 Provider 图标映射函数

**Files:**
- Modify: `crates/desktop/components/settings/provider-form-dialog.tsx`

**Step 1: 在文件顶部添加图标映射函数**

```typescript
import { BotIcon, SparklesIcon, UserIcon, SearchIcon, CloudIcon, MoonIcon } from "lucide-react";
import type { LucideIcon } from "lucide-react";

// 根据 provider 名称返回对应的图标
export function getProviderIcon(providerName: string, providerId: string): LucideIcon {
  const searchTarget = `${providerId} ${providerName}`.toLowerCase();

  if (searchTarget.includes("z.ai") || searchTarget.includes("z-ai") || searchTarget.includes("智谱")) {
    return BotIcon;
  }
  if (searchTarget.includes("openai") || searchTarget.includes("gpt")) {
    return SparklesIcon;
  }
  if (searchTarget.includes("anthropic") || searchTarget.includes("claude")) {
    return UserIcon;
  }
  if (searchTarget.includes("google") || searchTarget.includes("gemini")) {
    return SearchIcon;
  }
  if (searchTarget.includes("azure") || searchTarget.includes("microsoft") || searchTarget.includes("aws") || searchTarget.includes("amazon")) {
    return CloudIcon;
  }
  if (searchTarget.includes("moonshot") || searchTarget.includes("月之暗面")) {
    return MoonIcon;
  }
  if (searchTarget.includes("deepseek")) {
    return SearchIcon;
  }

  return CloudIcon;
}
```

**Step 2: 验证代码可以编译**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: 无错误

**Step 3: Commit**

```bash
cd /Users/wanyaozhong/projects/argusclaw/.worktrees/multi-model-tools
git add crates/desktop/components/settings/provider-form-dialog.tsx
git commit -m "feat: add provider icon mapping function"
```

---

## Task 2: 更新 ProviderCard 组件显示对应图标

**Files:**
- Modify: `crates/desktop/components/settings/provider-card.tsx:1-70`

**Step 1: 导入图标映射函数**

在文件顶部添加导入：
```typescript
import { getProviderIcon } from "./provider-form-dialog";
```

**Step 2: 替换 Cloud 图标为动态图标**

修改 CardTitle 部分：
```typescript
const ProviderIcon = getProviderIcon(provider.display_name, provider.id);

<CardTitle className="text-base flex items-center gap-2">
  <ProviderIcon className="h-5 w-5 text-muted-foreground" />
  <span>{provider.display_name}</span>
  {/* ... 其余代码保持不变 */}
</CardTitle>
```

**Step 3: 验证编译**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: 无错误

**Step 4: Commit**

```bash
git add crates/desktop/components/settings/provider-card.tsx
git commit -m "feat: use dynamic provider icon in card"
```

---

## Task 3: 重构 ProviderFormDialog 为分步对话框

**Files:**
- Modify: `crates/desktop/components/settings/provider-form-dialog.tsx`

**Step 1: 添加步骤状态**

在组件中添加步骤状态：
```typescript
type DialogStep = "provider" | "models";

const [step, setStep] = React.useState<DialogStep>("provider");
const [savedProviderId, setSavedProviderId] = React.useState<string | null>(provider?.id || null);
```

**Step 2: 重构 handleSubmit 为 handleSaveProvider**

```typescript
const handleSaveProvider = async () => {
  if (!formData.id.trim() || !formData.display_name.trim() || !formData.base_url.trim() || !formData.api_key.trim()) {
    return;
  }

  setSaving(true);
  try {
    await onSubmit(formData);
    setSavedProviderId(formData.id);
    setStep("models");
  } catch (error) {
    setModelError(error instanceof Error ? error.message : String(error));
  } finally {
    setSaving(false);
  }
};
```

**Step 3: 重构对话框内容为条件渲染**

```typescript
// 第一步：Provider 信息
{step === "provider" && (
  <>
    <DialogHeader>
      <DialogTitle>{isEditing ? "编辑 Provider" : "添加 Provider"}</DialogTitle>
      <DialogDescription>
        {isEditing ? "更新 LLM Provider 配置" : "配置一个新的 LLM Provider"}
      </DialogDescription>
    </DialogHeader>
    {/* 表单字段... */}
    <DialogFooter>
      <Button type="button" variant="outline" onClick={() => onOpenChange?.(false)}>
        取消
      </Button>
      <Button onClick={handleSaveProvider} disabled={saving || !canSave}>
        {saving ? "保存中..." : "下一步: 添加模型"}
      </Button>
    </DialogFooter>
  </>
)}

// 第二步：模型列表
{step === "models" && (
  <>
    <DialogHeader>
      <DialogTitle>添加模型</DialogTitle>
      <DialogDescription>
        为 {formData.display_name} 添加模型
      </DialogDescription>
    </DialogHeader>
    {/* 模型列表和添加表单... */}
    <DialogFooter>
      <Button type="button" variant="outline" onClick={() => setStep("provider")}>
        上一步
      </Button>
      <Button onClick={() => onOpenChange?.(false)}>
        完成
      </Button>
    </DialogFooter>
  </>
)}
```

**Step 4: 添加 canSave 验证**

```typescript
const canSave = Boolean(
  formData.id.trim() &&
  formData.display_name.trim() &&
  formData.base_url.trim() &&
  formData.api_key.trim()
);
```

**Step 5: 移除草稿模式相关代码**

- 删除 `draftModels` 状态
- 删除 `persistDraftModels` 函数
- 删除 `visibleModels` 中对 `draftModels` 的处理
- 删除对话框关闭时重置 `draftModels` 的代码

**Step 6: 修改打开时加载模型**

```typescript
React.useEffect(() => {
  if (savedProviderId) {
    loadPersistedModels(savedProviderId);
  }
}, [savedProviderId, loadPersistedModels]);
```

**Step 7: 验证编译**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: 无错误

**Step 8: Commit**

```bash
git add crates/desktop/components/settings/provider-form-dialog.tsx
git commit -m "refactor: convert provider dialog to multi-step with real-time save"
```

---

## Task 4: 将测试连接模型输入改为下拉选择

**Files:**
- Modify: `crates/desktop/components/settings/provider-form-dialog.tsx`

**Step 1: 添加模型选择状态**

```typescript
const [selectedModelForTest, setSelectedModelForTest] = React.useState<string>("");
```

**Step 2: 修改 handleTestConnection 使用选择的模型**

```typescript
const handleTestConnection = async () => {
  const record: ProviderInput = { ...formData };
  const modelName = selectedModelForTest || defaultModelName;

  if (!modelName) {
    setTestResult({
      provider_id: record.id,
      model: "",
      base_url: record.base_url,
      checked_at: new Date().toISOString(),
      latency_ms: 0,
      status: "model_not_available",
      message: "请先添加一个模型或选择模型",
    });
    setTestDialogOpen(true);
    return;
  }
  // ... 其余逻辑保持不变
};
```

**Step 3: 在测试连接区域添加模型下拉**

```typescript
{visibleModels.length > 0 && (
  <div className="space-y-2">
    <Label htmlFor="test_model">选择模型</Label>
    <select
      id="test_model"
      value={selectedModelForTest || defaultModelName}
      onChange={(e) => setSelectedModelForTest(e.target.value)}
      className="flex h-7 w-full rounded-md border border-input bg-input/20 px-2 py-0.5 text-sm"
    >
      {visibleModels.map((model) => (
        <option key={model.id} value={model.name}>
          {model.name} {model.is_default ? "(默认)" : ""}
        </option>
      ))}
    </select>
  </div>
)}
```

**Step 4: 验证编译**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: 无错误

**Step 5: Commit**

```bash
git add crates/desktop/components/settings/provider-form-dialog.tsx
git commit -m "feat: use dropdown for model selection in test connection"
```

---

## Task 5: 验证端到端功能

**Step 1: 启动开发服务器**

Run: `cd crates/desktop && pnpm dev`
Expected: 开发服务器启动成功

**Step 2: 测试创建 Provider 流程**

1. 打开 `/settings/providers` 页面
2. 点击 "Add Provider"
3. 填写 ID、名称、Base URL、API Key
4. 点击 "下一步: 添加模型"
5. 添加一个模型
6. 点击 "完成"
7. 验证 Provider 卡片显示正确图标
8. 验证模型已保存（刷新页面后仍在）

**Step 3: 测试编辑 Provider 流程**

1. 点击已有 Provider 的编辑按钮
2. 验证直接进入模型步骤
3. 添加新模型
4. 验证模型实时保存

**Step 4: 测试连接功能**

1. 点击 "测试连接" 按钮
2. 验证下拉显示已配置的模型
3. 选择模型并测试

**Step 5: Commit**

```bash
git add -A
git commit -m "test: verify provider and model configuration flow"
```

---

## 执行选择

**Plan complete and saved to `docs/plans/2026-03-16-provider-model-dialog-refactor.md`. Two execution options:**

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
