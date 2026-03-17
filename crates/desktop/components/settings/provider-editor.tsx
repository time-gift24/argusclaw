"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import {
  ArrowLeft,
  CircleAlert,
  CircleCheckBig,
  LoaderCircle,
  Save,
} from "lucide-react";
import {
  providers,
  type LlmProviderRecord,
  type ProviderModelConfig,
  type ProviderInput,
  type ProviderTestResult,
} from "@/lib/tauri";
import { Breadcrumb } from "@/components/settings";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface ProviderEditorProps {
  providerId?: string;
}

function createDefaultFormData(): LlmProviderRecord {
  return {
    id: "",
    kind: "openai-compatible",
    display_name: "",
    base_url: "",
    api_key: "",
    models: [],
    default_model: "",
    model_config: {},
    is_default: false,
    extra_headers: {},
    secret_status: "ready",
  };
}

function normalizeModelConfig(
  models: string[],
  modelConfig: Record<string, ProviderModelConfig>,
): Record<string, ProviderModelConfig> {
  return Object.fromEntries(
    models.map((model) => {
      const contextLength = modelConfig[model]?.context_length;

      return [
        model,
        typeof contextLength === "number" ? { context_length: contextLength } : {},
      ];
    }),
  );
}

function toProviderInput(record: LlmProviderRecord): ProviderInput {
  return {
    id: record.id.trim() || undefined,
    kind: record.kind,
    display_name: record.display_name,
    base_url: record.base_url,
    api_key: record.api_key,
    models: record.models,
    default_model: record.default_model,
    model_config: normalizeModelConfig(record.models, record.model_config),
    is_default: record.is_default,
    extra_headers: record.extra_headers,
  };
}

function formatCheckedAt(value: string) {
  return new Date(value).toLocaleString("zh-CN", {
    hour12: false,
  });
}

function parseContextLength(value: string): number | undefined {
  if (!value.trim()) {
    return undefined;
  }

  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return undefined;
  }

  return parsed;
}

export function ProviderEditor({ providerId }: ProviderEditorProps) {
  const router = useRouter();
  const isEditing = Boolean(providerId);

  const [loading, setLoading] = React.useState(isEditing);
  const [saving, setSaving] = React.useState(false);
  const [testingConnection, setTestingConnection] = React.useState(false);
  const [testResult, setTestResult] = React.useState<ProviderTestResult | null>(
    null,
  );
  const [newModel, setNewModel] = React.useState("");
  const [testSelectedModel, setTestSelectedModel] = React.useState<string>("");
  const [formData, setFormData] = React.useState<LlmProviderRecord>(
    () => createDefaultFormData(),
  );

  const selectedTestModel =
    testSelectedModel || formData.default_model || formData.models[0] || "";
  const canSave = Boolean(
    formData.display_name.trim() &&
      formData.base_url.trim() &&
      formData.api_key.trim() &&
      formData.models.length > 0 &&
      formData.default_model.trim(),
  );
  const canTest = Boolean(
    formData.base_url.trim() &&
      formData.api_key.trim() &&
      formData.models.length > 0 &&
      selectedTestModel,
  );

  React.useEffect(() => {
    const loadProvider = async () => {
      if (!providerId) {
        setLoading(false);
        return;
      }

      try {
        const record = await providers.get(providerId);
        if (!record) {
          router.push("/settings/providers");
          return;
        }

        setFormData({
          ...record,
          model_config: normalizeModelConfig(record.models, record.model_config),
        });
        setTestSelectedModel(record.default_model || record.models[0] || "");
      } catch (error) {
        console.error("Failed to load provider:", error);
      } finally {
        setLoading(false);
      }
    };

    void loadProvider();
  }, [providerId, router]);

  React.useEffect(() => {
    const fallbackModel = formData.default_model || formData.models[0] || "";
    setTestSelectedModel((current) => {
      if (current && formData.models.includes(current)) {
        return current;
      }
      return fallbackModel;
    });
  }, [formData.default_model, formData.models]);

  const handleAddModel = React.useCallback(() => {
    const trimmed = newModel.trim();
    if (!trimmed) return;

    setFormData((prev) => {
      if (prev.models.includes(trimmed)) {
        return prev;
      }

      const models = [...prev.models, trimmed];
      return {
        ...prev,
        models,
        default_model: prev.default_model || trimmed,
      };
    });
    setTestSelectedModel((current) => current || trimmed);
    setNewModel("");
  }, [newModel]);

  const handleRemoveModel = React.useCallback((modelToRemove: string) => {
    setFormData((prev) => {
      const models = prev.models.filter((model) => model !== modelToRemove);
      const defaultModel = models.includes(prev.default_model)
        ? prev.default_model
        : models[0] || "";

      return {
        ...prev,
        models,
        default_model: defaultModel,
      };
    });
  }, []);

  const handleSetDefaultModel = React.useCallback((model: string) => {
    setFormData((prev) => ({ ...prev, default_model: model }));
    setTestSelectedModel(model);
  }, []);

  const handleContextLengthChange = React.useCallback(
    (model: string, value: string) => {
      const contextLength = parseContextLength(value);
      setFormData((prev) => ({
        ...prev,
        model_config: {
          ...prev.model_config,
          [model]:
            typeof contextLength === "number"
              ? { context_length: contextLength }
              : {},
        },
      }));
    },
    [],
  );

  const handleSave = async () => {
    if (!canSave) {
      return;
    }

    setSaving(true);
    try {
      await providers.upsert(toProviderInput(formData));
      router.push("/settings/providers");
    } catch (error) {
      console.error("Failed to save provider:", error);
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    if (!canTest) {
      return;
    }

    const record = toProviderInput(formData);
    setTestingConnection(true);
    setTestResult(null);
    try {
      const result = await providers.testInput(record, selectedTestModel);
      setTestResult(result);
    } catch (error) {
      setTestResult({
        provider_id: record.id ?? "draft",
        model: selectedTestModel,
        base_url: record.base_url,
        checked_at: new Date().toISOString(),
        latency_ms: 0,
        status: "request_failed",
        message: error instanceof Error ? error.message : String(error),
      });
      console.error("Failed to test provider draft:", error);
    } finally {
      setTestingConnection(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    );
  }

  const statusTone = testingConnection
    ? "border-sky-200 text-sky-700"
    : testResult?.status === "success"
      ? "border-emerald-200 text-emerald-700"
      : "border-destructive/30 text-destructive";
  const statusLabel = testingConnection
    ? "运行中"
    : testResult?.status === "success"
      ? "成功"
      : "待测试";

  return (
    <div className="w-full mx-auto max-w-7xl px-6 py-6 space-y-4">
      <Breadcrumb
        items={[
          { label: "设置", href: "/settings" },
          { label: "LLM 提供者", href: "/settings/providers" },
          { label: isEditing ? formData.display_name || providerId || "编辑" : "新建" },
        ]}
      />

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" onClick={() => router.back()}>
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h1 className="text-sm font-semibold">
            {isEditing ? "编辑 Provider" : "新增 Provider"}
          </h1>
        </div>
        <Button size="sm" onClick={handleSave} disabled={saving || !canSave}>
          <Save className="h-4 w-4 mr-1" />
          {saving ? "保存中..." : "保存"}
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-6 min-h-[calc(100vh-200px)]">
        <div className="space-y-4 overflow-y-auto pr-2">
          {formData.secret_status === "requires_reentry" && (
            <div className="rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-sm text-amber-900">
              当前保存的密钥已无法解密，请重新填写 API Key 后再保存。
            </div>
          )}

          <div className="space-y-2">
            <Label htmlFor="display_name">名称</Label>
            <Input
              id="display_name"
              value={formData.display_name}
              onChange={(event) =>
                setFormData({ ...formData, display_name: event.target.value })
              }
              placeholder="我的 LLM Provider"
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="kind">Kind</Label>
            <select
              id="kind"
              value={formData.kind}
              onChange={(event) =>
                setFormData({
                  ...formData,
                  kind: event.target.value as ProviderInput["kind"],
                })
              }
              className="flex h-7 w-full rounded-md border border-input bg-input/20 px-2 py-0.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 dark:bg-input/30"
            >
              <option value="openai-compatible">openai-compatible</option>
            </select>
            <p className="text-[11px] text-muted-foreground">
              当前桌面端仅开放 OpenAI Compatible 类型。
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="base_url">Base URL</Label>
            <Input
              id="base_url"
              value={formData.base_url}
              onChange={(event) =>
                setFormData({ ...formData, base_url: event.target.value })
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
              onChange={(event) =>
                setFormData({ ...formData, api_key: event.target.value })
              }
              placeholder="sk-..."
              required
            />
          </div>

          <div className="space-y-2">
            <Label>Models</Label>
            <div className="flex flex-wrap gap-2">
              {formData.models.map((model) => (
                <Badge
                  key={model}
                  variant={model === formData.default_model ? "default" : "secondary"}
                  className="cursor-pointer pr-1"
                  onClick={() => handleSetDefaultModel(model)}
                >
                  {model}
                  {model === formData.default_model ? (
                    <span className="ml-1 text-[10px] opacity-70">默认</span>
                  ) : null}
                  <button
                    type="button"
                    className="ml-1 hover:text-destructive"
                    onClick={(event) => {
                      event.stopPropagation();
                      handleRemoveModel(model);
                    }}
                  >
                    ×
                  </button>
                </Badge>
              ))}
            </div>
            <div className="flex gap-2">
              <Input
                value={newModel}
                onChange={(event) => setNewModel(event.target.value)}
                placeholder="输入模型名称"
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    handleAddModel();
                  }
                }}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddModel}
                disabled={!newModel.trim()}
              >
                添加
              </Button>
            </div>
            <p className="text-[11px] text-muted-foreground">
              点击模型标签可设为默认模型，同时会作为默认测试模型。
            </p>
          </div>

          {formData.models.length > 0 ? (
            <div className="space-y-3 rounded-lg border border-border/60 p-4">
              <div>
                <p className="text-sm font-medium">模型配置</p>
                <p className="text-[11px] text-muted-foreground">
                  为每个模型单独配置最大上下文，聊天页会优先使用这里的值。
                </p>
              </div>

              <div className="space-y-3">
                {formData.models.map((model) => (
                  <div
                    key={`config-${model}`}
                    className="grid gap-3 rounded-lg border border-border/60 p-3 md:grid-cols-[minmax(0,1fr)_220px]"
                  >
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{model}</span>
                        {model === formData.default_model ? (
                          <Badge variant="secondary">默认模型</Badge>
                        ) : null}
                      </div>
                      <p className="text-[11px] text-muted-foreground">
                        留空时聊天页会使用默认 128000。
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`context_length_${model}`}>最大上下文</Label>
                      <Input
                        id={`context_length_${model}`}
                        type="number"
                        min={1}
                        step={1}
                        value={formData.model_config[model]?.context_length?.toString() ?? ""}
                        onChange={(event) =>
                          handleContextLengthChange(model, event.target.value)
                        }
                        placeholder="例如 128000"
                      />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </div>

        <div className="border rounded-lg overflow-hidden flex flex-col">
          <div className="bg-muted/50 px-4 py-2 border-b text-xs font-medium text-muted-foreground">
            连接测试
          </div>
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            <div className="rounded-lg border border-border/60 bg-muted/30 p-4">
              <div className="flex items-start justify-between gap-3">
                <div className="space-y-1">
                  <p className="text-sm font-medium">
                    {formData.display_name || "未命名 Provider"}
                  </p>
                  <p className="font-mono text-[11px] text-muted-foreground">
                    {formData.id || "draft-provider"}
                  </p>
                </div>
                <Badge variant="outline" className={statusTone}>
                  {testingConnection ? (
                    <LoaderCircle className="mr-1 h-3 w-3 animate-spin" />
                  ) : testResult?.status === "success" ? (
                    <CircleCheckBig className="mr-1 h-3 w-3" />
                  ) : (
                    <CircleAlert className="mr-1 h-3 w-3" />
                  )}
                  {statusLabel}
                </Badge>
              </div>

              <div className="mt-4 grid gap-3 text-xs">
                <div className="flex items-start justify-between gap-3">
                  <span className="text-muted-foreground">Model</span>
                  <span className="font-mono text-right">
                    {selectedTestModel || "-"}
                  </span>
                </div>
                <div className="flex items-start justify-between gap-3">
                  <span className="text-muted-foreground">Base URL</span>
                  <span className="max-w-[300px] break-all font-mono text-right">
                    {formData.base_url || "-"}
                  </span>
                </div>
                <div className="flex items-start justify-between gap-3">
                  <span className="text-muted-foreground">最大上下文</span>
                  <span className="font-mono text-right">
                    {selectedTestModel
                      ? formData.model_config[selectedTestModel]?.context_length ?? 128000
                      : "-"}
                  </span>
                </div>
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="test_model" className="text-xs text-muted-foreground">
                选择测试模型
              </Label>
              <select
                id="test_model"
                value={selectedTestModel}
                onChange={(event) => setTestSelectedModel(event.target.value)}
                className="flex h-7 w-full rounded-md border border-input bg-input/20 px-2 py-0.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 dark:bg-input/30"
                disabled={formData.models.length === 0}
              >
                {formData.models.length === 0 ? (
                  <option value="">请先添加模型</option>
                ) : (
                  formData.models.map((model) => (
                    <option key={model} value={model}>
                      {model}
                      {model === formData.default_model ? " (默认)" : ""}
                    </option>
                  ))
                )}
              </select>
            </div>

            <div className="rounded-lg border border-border/60 p-4">
              <p className="mb-2 text-xs font-medium text-muted-foreground">
                详情
              </p>
              <p className="text-sm">
                {testingConnection
                  ? "正在测试当前草稿配置的连接状态，请稍候。"
                  : testResult?.message || "保存前可以先验证当前草稿配置是否可连通。"}
              </p>
            </div>

            <div className="grid gap-3 rounded-lg border border-border/60 p-4 text-xs">
              <div className="flex items-center justify-between gap-3">
                <span className="font-mono text-muted-foreground">
                  latency_ms
                </span>
                <span className="font-mono">
                  {testResult ? `${testResult.latency_ms} ms` : "-"}
                </span>
              </div>
              <div className="flex items-center justify-between gap-3">
                <span className="font-mono text-muted-foreground">
                  checked_at
                </span>
                <span className="font-mono">
                  {testResult ? formatCheckedAt(testResult.checked_at) : "-"}
                </span>
              </div>
            </div>

            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => router.push("/settings/providers")}
              >
                返回列表
              </Button>
              <Button
                onClick={() => {
                  void handleTestConnection();
                }}
                disabled={testingConnection || !canTest}
              >
                {testingConnection ? "正在测试" : testResult ? "重新测试" : "测试连接"}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
