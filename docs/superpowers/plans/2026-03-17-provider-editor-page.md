# Provider 编辑页面实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Provider 的编辑和添加功能从弹窗改为独立页面，左侧配置项，右侧模型可达性测试面板。

**Architecture:** 采用与 Agent 编辑页面一致的左右两栏布局。新建可复用的 `ProviderEditor` 组件和 `ProviderTestPanel` 组件，通过 Next.js 路由 `/settings/providers/new` 和 `/settings/providers/[id]` 访问。

**Tech Stack:** React 19, TypeScript, Next.js, Tailwind CSS v4, shadcn/ui

---

## 文件结构

| 文件 | 操作 | 职责 |
|------|------|------|
| `components/settings/provider-editor.tsx` | 创建 | 主编辑组件，左右布局 |
| `components/settings/provider-test-panel.tsx` | 创建 | 右侧测试面板，模型测试列表 |
| `components/settings/provider-model-list.tsx` | 创建 | 模型列表输入组件（左侧） |
| `app/settings/providers/new/page.tsx` | 创建 | 新建页面入口 |
| `app/settings/providers/[id]/page.tsx` | 创建 | 编辑页面入口 |
| `components/settings/provider-card.tsx` | 修改 | 编辑按钮跳转到独立页面 |
| `components/settings/index.ts` | 修改 | 导出新组件 |
| `app/settings/providers/page.tsx` | 修改 | 新建按钮跳转到独立页面 |

---

## Chunk 1: 模型列表组件

### Task 1: ProviderModelList 组件

**Files:**
- Create: `crates/desktop/components/settings/provider-model-list.tsx`

- [ ] **Step 1: 创建 ProviderModelList 组件**

```tsx
"use client";

import * as React from "react";
import { Plus } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

interface ProviderModelListProps {
  models: string[];
  defaultModel: string;
  onModelsChange: (models: string[]) => void;
  onDefaultModelChange: (model: string) => void;
  disabled?: boolean;
}

export function ProviderModelList({
  models,
  defaultModel,
  onModelsChange,
  onDefaultModelChange,
  disabled = false,
}: ProviderModelListProps) {
  const [newModel, setNewModel] = React.useState("");

  const handleAddModel = React.useCallback(() => {
    const trimmed = newModel.trim();
    if (!trimmed || models.includes(trimmed)) return;
    const newModels = [...models, trimmed];
    onModelsChange(newModels);
    if (!defaultModel) {
      onDefaultModelChange(trimmed);
    }
    setNewModel("");
  }, [newModel, models, defaultModel, onModelsChange, onDefaultModelChange]);

  const handleRemoveModel = React.useCallback(
    (model: string) => {
      const newModels = models.filter((m) => m !== model);
      onModelsChange(newModels);
      if (defaultModel === model) {
        onDefaultModelChange(newModels[0] || "");
      }
    },
    [models, defaultModel, onModelsChange, onDefaultModelChange]
  );

  const handleKeyDown = React.useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAddModel();
      }
    },
    [handleAddModel]
  );

  return (
    <div className="space-y-2">
      <label className="text-xs text-muted-foreground">模型列表</label>
      {models.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {models.map((model) => (
            <Badge
              key={model}
              variant={model === defaultModel ? "default" : "secondary"}
              className="cursor-pointer pr-1"
              onClick={() => onDefaultModelChange(model)}
            >
              {model}
              {model === defaultModel && (
                <span className="ml-1 text-[10px] opacity-70">默认</span>
              )}
              {!disabled && (
                <button
                  type="button"
                  className="ml-1 hover:text-destructive"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleRemoveModel(model);
                  }}
                >
                  ×
                </button>
              )}
            </Badge>
          ))}
        </div>
      )}
      {!disabled && (
        <div className="flex gap-2">
          <Input
            value={newModel}
            onChange={(e) => setNewModel(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="输入模型名称"
            disabled={disabled}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddModel}
            disabled={!newModel.trim() || disabled}
          >
            <Plus className="h-4 w-4" />
          </Button>
        </div>
      )}
      <p className="text-[11px] text-muted-foreground">点击标签设为默认模型</p>
    </div>
  );
}
```

- [ ] **Step 2: 提交组件**

```bash
git add crates/desktop/components/settings/provider-model-list.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add ProviderModelList component

Reusable component for managing model list with default model selection.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 2: 测试面板组件

### Task 2: ProviderTestPanel 组件

**Files:**
- Create: `crates/desktop/components/settings/provider-test-panel.tsx`

- [ ] **Step 1: 定义测试状态类型和组件**

```tsx
"use client";

import * as React from "react";
import {
  CircleAlert,
  CircleCheckBig,
  LoaderCircle,
  RefreshCw,
} from "lucide-react";
import type { ProviderTestResult, ProviderTestStatus } from "@/lib/tauri";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export type ModelTestState = {
  model: string;
  status: "idle" | "testing" | "success" | "error";
  result?: ProviderTestResult;
};

interface ProviderTestPanelProps {
  models: string[];
  defaultModel: string;
  providerId?: string;
  getTestInput: () => {
    id: string;
    kind: "openai-compatible";
    display_name: string;
    base_url: string;
    api_key: string;
    models: string[];
    default_model: string;
    is_default: boolean;
    extra_headers: Record<string, string>;
  } | null;
  canTest: boolean;
}

const statusColors: Record<string, string> = {
  success: "text-emerald-600",
  auth_failed: "text-red-600",
  model_not_available: "text-red-600",
  rate_limited: "text-amber-600",
  request_failed: "text-red-600",
  invalid_response: "text-red-600",
  provider_not_found: "text-red-600",
  unsupported_provider_kind: "text-red-600",
};

const statusLabels: Record<ProviderTestStatus, string> = {
  success: "成功",
  auth_failed: "认证失败",
  model_not_available: "模型不可用",
  rate_limited: "请求限制",
  request_failed: "请求失败",
  invalid_response: "响应无效",
  provider_not_found: "Provider 未找到",
  unsupported_provider_kind: "不支持的类型",
};

export function ProviderTestPanel({
  models,
  defaultModel,
  providerId,
  getTestInput,
  canTest,
}: ProviderTestPanelProps) {
  const [testStates, setTestStates] = React.useState<Record<string, ModelTestState>>({});
  const [testingAll, setTestingAll] = React.useState(false);
  const [expandedError, setExpandedError] = React.useState<string | null>(null);

  const runTest = React.useCallback(
    async (model: string) => {
      const input = getTestInput();
      if (!input) return;

      setTestStates((prev) => ({
        ...prev,
        [model]: { model, status: "testing" },
      }));

      try {
        const { providers } = await import("@/lib/tauri");
        const result = providerId
          ? await providers.testConnection(providerId, model)
          : await providers.testInput(input, model);

        setTestStates((prev) => ({
          ...prev,
          [model]: { model, status: result.status === "success" ? "success" : "error", result },
        }));
      } catch (error) {
        setTestStates((prev) => ({
          ...prev,
          [model]: {
            model,
            status: "error",
            result: {
              provider_id: input.id,
              model,
              base_url: input.base_url,
              checked_at: new Date().toISOString(),
              latency_ms: 0,
              status: "request_failed",
              message: error instanceof Error ? error.message : String(error),
            },
          },
        }));
      }
    },
    [getTestInput, providerId]
  );

  const testAll = React.useCallback(async () => {
    if (!canTest || models.length === 0) return;
    setTestingAll(true);
    for (const model of models) {
      await runTest(model);
    }
    setTestingAll(false);
  }, [canTest, models, runTest]);

  // Auto-test when models change
  React.useEffect(() => {
    if (!canTest) return;
    const newModels = models.filter((m) => !testStates[m]);
    if (newModels.length > 0) {
      void runTest(newModels[0]);
    }
  }, [models, canTest, runTest, testStates]);

  const successCount = Object.values(testStates).filter(
    (s) => s.status === "success"
  ).length;
  const avgLatency =
    successCount > 0
      ? Math.round(
          Object.values(testStates)
            .filter((s) => s.status === "success" && s.result)
            .reduce((sum, s) => sum + (s.result?.latency_ms || 0), 0) / successCount
        )
      : null;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground font-medium uppercase">
          模型可达性测试
        </span>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void testAll()}
          disabled={!canTest || testingAll || models.length === 0}
        >
          {testingAll ? (
            <LoaderCircle className="h-3 w-3 mr-1 animate-spin" />
          ) : (
            <RefreshCw className="h-3 w-3 mr-1" />
          )}
          全部测试
        </Button>
      </div>

      {models.length === 0 ? (
        <div className="text-sm text-muted-foreground py-8 text-center border rounded-lg border-dashed">
          添加模型后可进行可达性测试
        </div>
      ) : (
        <div className="border rounded-lg overflow-hidden">
          {models.map((model) => {
            const state = testStates[model];
            const isDefault = model === defaultModel;

            return (
              <div key={model}>
                <div
                  className={`px-3 py-2 flex items-center justify-between cursor-pointer hover:bg-muted/50 ${
                    state?.status === "error" ? "bg-red-50" : ""
                  } ${state?.status === "testing" ? "bg-blue-50" : ""}`}
                  onClick={() => state?.status !== "testing" && void runTest(model)}
                >
                  <div className="flex items-center gap-2">
                    {state?.status === "testing" ? (
                      <LoaderCircle className="h-4 w-4 text-blue-500 animate-spin" />
                    ) : state?.status === "success" ? (
                      <CircleCheckBig className="h-4 w-4 text-emerald-500" />
                    ) : state?.status === "error" ? (
                      <CircleAlert className="h-4 w-4 text-red-500" />
                    ) : (
                      <CircleAlert className="h-4 w-4 text-muted-foreground/50" />
                    )}
                    <span className="font-mono text-sm">{model}</span>
                    {isDefault && (
                      <Badge variant="secondary" className="text-[10px]">
                        默认
                      </Badge>
                    )}
                  </div>
                  <div className="text-xs">
                    {state?.status === "testing" ? (
                      <span className="text-blue-600">测试中...</span>
                    ) : state?.status === "success" && state.result ? (
                      <span className="text-emerald-600 font-mono">
                        {state.result.latency_ms}ms
                      </span>
                    ) : state?.status === "error" && state.result ? (
                      <span
                        className="text-red-600 cursor-pointer underline"
                        onClick={(e) => {
                          e.stopPropagation();
                          setExpandedError(expandedError === model ? null : model);
                        }}
                      >
                        {statusLabels[state.result.status]}
                      </span>
                    ) : (
                      <span className="text-muted-foreground">待测试</span>
                    )}
                  </div>
                </div>

                {state?.status === "error" && expandedError === model && state.result && (
                  <div className="px-3 py-2 bg-red-50 border-t text-xs">
                    <div className="font-medium text-red-700 mb-1">
                      {model} 错误详情
                    </div>
                    <div className="font-mono text-red-600 whitespace-pre-wrap">
                      {state.result.message}
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {successCount > 0 && (
        <div className="flex items-center justify-between text-xs px-3 py-2 bg-emerald-50 border border-emerald-200 rounded-lg">
          <span>
            测试结果: {successCount}/{models.length} 通过
          </span>
          {avgLatency !== null && (
            <span className="text-emerald-600 font-mono">
              平均延迟: {avgLatency}ms
            </span>
          )}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: 提交组件**

```bash
git add crates/desktop/components/settings/provider-test-panel.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add ProviderTestPanel component

Test panel for model connectivity testing with auto-test on model add,
manual test all, and error detail expansion.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 3: 主编辑组件

### Task 3: ProviderEditor 组件

**Files:**
- Create: `crates/desktop/components/settings/provider-editor.tsx`

- [ ] **Step 1: 创建 ProviderEditor 主组件**

```tsx
"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { ArrowLeft, Save } from "lucide-react";
import { providers, type LlmProviderSummary, type ProviderSecretStatus } from "@/lib/tauri";

import { Breadcrumb } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { ProviderModelList } from "./provider-model-list";
import { ProviderTestPanel } from "./provider-test-panel";

interface ProviderFormData {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

function createDefaultFormData(): ProviderFormData {
  return {
    id: "",
    kind: "openai-compatible",
    display_name: "",
    base_url: "",
    api_key: "",
    models: [],
    default_model: "",
    is_default: false,
    extra_headers: {},
    secret_status: "ready",
  };
}

interface ProviderEditorProps {
  providerId?: string;
}

export function ProviderEditor({ providerId }: ProviderEditorProps) {
  const router = useRouter();
  const isEditing = !!providerId;

  const [loading, setLoading] = React.useState(isEditing);
  const [saving, setSaving] = React.useState(false);
  const [formData, setFormData] = React.useState<ProviderFormData>(createDefaultFormData());

  const canSave = Boolean(
    formData.id.trim() &&
    formData.display_name.trim() &&
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    formData.models.length > 0
  );

  const canTest = Boolean(
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    formData.models.length > 0
  );

  // Load provider data if editing
  React.useEffect(() => {
    if (!providerId) {
      setLoading(false);
      return;
    }

    const loadProvider = async () => {
      try {
        const provider = await providers.get(providerId);
        if (provider) {
          setFormData({
            id: provider.id,
            kind: provider.kind,
            display_name: provider.display_name,
            base_url: provider.base_url,
            api_key: typeof provider.api_key === "string"
              ? provider.api_key
              : (provider.api_key as { api_key: string }).api_key || "",
            models: provider.models,
            default_model: provider.default_model,
            is_default: provider.is_default,
            extra_headers: provider.extra_headers,
            secret_status: provider.secret_status,
          });
        }
      } catch (error) {
        console.error("Failed to load provider:", error);
      } finally {
        setLoading(false);
      }
    };

    void loadProvider();
  }, [providerId]);

  const getTestInput = React.useCallback(() => {
    if (!canTest) return null;
    return {
      id: formData.id,
      kind: formData.kind,
      display_name: formData.display_name,
      base_url: formData.base_url,
      api_key: formData.api_key,
      models: formData.models,
      default_model: formData.default_model,
      is_default: formData.is_default,
      extra_headers: formData.extra_headers,
    };
  }, [formData, canTest]);

  const handleSave = async () => {
    if (!canSave) return;

    setSaving(true);
    try {
      await providers.upsert({
        id: formData.id,
        kind: formData.kind,
        display_name: formData.display_name,
        base_url: formData.base_url,
        api_key: formData.api_key,
        models: formData.models,
        default_model: formData.default_model,
        is_default: formData.is_default,
        extra_headers: formData.extra_headers,
      });
      router.push("/settings/providers");
    } catch (error) {
      console.error("Failed to save provider:", error);
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-7xl px-6 py-6 space-y-4">
      <Breadcrumb
        items={[
          { label: "设置", href: "/settings" },
          { label: "LLM 提供者", href: "/settings/providers" },
          ...(isEditing
            ? [{ label: formData.display_name || "编辑" }]
            : [{ label: "新建" }]),
        ]}
      />

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" onClick={() => router.back()}>
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h1 className="text-sm font-semibold">
            {isEditing ? "编辑 Provider" : "新建 Provider"}
          </h1>
        </div>
        <Button size="sm" onClick={handleSave} disabled={saving || !canSave}>
          <Save className="h-4 w-4 mr-1" />
          {saving ? "保存中..." : "保存"}
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-6 min-h-[calc(100vh-200px)]">
        {/* Left: Form */}
        <div className="space-y-4 overflow-y-auto pr-2">
          {formData.secret_status === "requires_reentry" && (
            <div className="rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-sm text-amber-900">
              当前保存的密钥已无法解密，请重新填写 API Key 后再保存。
            </div>
          )}

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="id">ID</Label>
              <Input
                id="id"
                value={formData.id}
                onChange={(e) => setFormData({ ...formData, id: e.target.value })}
                placeholder="unique-provider-id"
                required
                disabled={isEditing}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="display_name">名称</Label>
              <Input
                id="display_name"
                value={formData.display_name}
                onChange={(e) =>
                  setFormData({ ...formData, display_name: e.target.value })
                }
                placeholder="My Provider"
                required
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="base_url">Base URL</Label>
            <Input
              id="base_url"
              value={formData.base_url}
              onChange={(e) =>
                setFormData({ ...formData, base_url: e.target.value })
              }
              placeholder="https://api.example.com/v1"
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="api_key">API Key</Label>
            <Input
              id="api_key"
              type="password"
              value={formData.api_key}
              onChange={(e) =>
                setFormData({ ...formData, api_key: e.target.value })
              }
              placeholder="sk-..."
              required
            />
          </div>

          <ProviderModelList
            models={formData.models}
            defaultModel={formData.default_model}
            onModelsChange={(models) => setFormData({ ...formData, models })}
            onDefaultModelChange={(default_model) =>
              setFormData({ ...formData, default_model })
            }
          />

          <div className="flex items-center space-x-2 pt-2">
            <Checkbox
              id="is_default"
              checked={formData.is_default}
              onCheckedChange={(checked) =>
                setFormData({ ...formData, is_default: checked === true })
              }
            />
            <Label htmlFor="is_default" className="cursor-pointer">
              设为默认 Provider
            </Label>
          </div>
        </div>

        {/* Right: Test Panel */}
        <div className="border-l pl-6">
          <ProviderTestPanel
            models={formData.models}
            defaultModel={formData.default_model}
            providerId={isEditing ? providerId : undefined}
            getTestInput={getTestInput}
            canTest={canTest}
          />
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 提交组件**

```bash
git add crates/desktop/components/settings/provider-editor.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add ProviderEditor component

Main editor component with left-side form and right-side test panel.
Supports both create and edit modes.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 4: 路由页面和集成

### Task 4: 创建路由页面

**Files:**
- Create: `crates/desktop/app/settings/providers/new/page.tsx`
- Create: `crates/desktop/app/settings/providers/[id]/page.tsx`

- [ ] **Step 1: 创建新建页面**

```tsx
"use client";

import { ProviderEditor } from "@/components/settings";

export default function NewProviderPage() {
  return <ProviderEditor />;
}
```

- [ ] **Step 2: 创建编辑页面**

```tsx
import { ProviderEditor } from "@/components/settings";

export default async function EditProviderPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return <ProviderEditor providerId={id} />;
}
```

- [ ] **Step 3: 提交路由页面**

```bash
git add crates/desktop/app/settings/providers/new/page.tsx
git add crates/desktop/app/settings/providers/[id]/page.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add provider editor routes

Add /settings/providers/new and /settings/providers/[id] routes
for creating and editing providers.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

### Task 5: 更新导出和导航

**Files:**
- Modify: `crates/desktop/components/settings/index.ts`
- Modify: `crates/desktop/components/settings/provider-card.tsx`
- Modify: `crates/desktop/app/settings/providers/page.tsx`

- [ ] **Step 1: 更新 index.ts 导出**

在 `crates/desktop/components/settings/index.ts` 中添加新组件导出：

```tsx
export { ProviderCard, type LlmProviderSummary } from "./provider-card";
export {
  ProviderFormDialog,
  type LlmProviderRecord,
} from "./provider-form-dialog";
export { ProviderTestDialog } from "./provider-test-dialog";
export { AgentCard, type AgentRecord } from "./agent-card";
export { AgentFormDialog } from "./agent-form-dialog";
export { DeleteConfirmDialog } from "./delete-confirm-dialog";
export { Breadcrumb } from "./breadcrumb";
export { AgentEditor } from "./agent-editor";
export { ProviderEditor } from "./provider-editor";
export { ProviderTestPanel } from "./provider-test-panel";
export { ProviderModelList } from "./provider-model-list";
```

- [ ] **Step 2: 修改 ProviderCard 编辑按钮**

在 `provider-card.tsx` 中，将 `onEdit` 改为使用路由跳转：

找到 `onEdit` 调用的位置，改为：

```tsx
import { useRouter } from "next/navigation";

// 在组件内
const router = useRouter();

// 修改编辑按钮点击
onClick={() => router.push(`/settings/providers/${provider.id}`)}
```

- [ ] **Step 3: 修改 Provider 列表页新建按钮**

在 `app/settings/providers/page.tsx` 中，将新建按钮改为跳转：

找到 `ProviderFormDialog` 的使用位置，改为使用 `Button` + `router.push`：

```tsx
import { useRouter } from "next/navigation";
import { Plus } from "lucide-react";

// 在组件内
const router = useRouter();

// 替换 ProviderFormDialog trigger 为
<Button size="sm" onClick={() => router.push("/settings/providers/new")}>
  <Plus className="h-4 w-4 mr-1" />
  Add Provider
</Button>
```

- [ ] **Step 4: 提交导航修改**

```bash
git add crates/desktop/components/settings/index.ts
git add crates/desktop/components/settings/provider-card.tsx
git add crates/desktop/app/settings/providers/page.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): integrate provider editor with navigation

- Export new components from index
- Change ProviderCard edit button to navigate to editor page
- Change providers list page to navigate to new provider page

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## 验收清单

- [ ] 访问 `/settings/providers/new` 显示新建 Provider 页面
- [ ] 访问 `/settings/providers/[id]` 显示编辑 Provider 页面
- [ ] 左侧表单字段与原有弹窗一致
- [ ] 添加模型后自动触发测试
- [ ] 点击"全部测试"按钮测试所有模型
- [ ] 测试状态正确显示（成功/失败/测试中）
- [ ] 失败项可展开查看错误详情
- [ ] 底部显示测试汇总
- [ ] 保存后跳转回列表页
- [ ] ProviderCard 编辑按钮跳转到编辑页
- [ ] 列表页新建按钮跳转到新建页
