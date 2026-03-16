"use client";

import * as React from "react";
import { Plus, Pencil, Trash2, Star, BotIcon, SparklesIcon, UserIcon, SearchIcon, CloudIcon, MoonIcon } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  providers,
  models,
  type ProviderInput,
  type ProviderSecretStatus,
  type ProviderTestResult,
  type LlmModelRecord,
  type ModelInput,
} from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProviderTestDialog } from "./provider-test-dialog";

export interface LlmProviderRecord {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

interface ProviderFormDialogProps {
  provider?: LlmProviderRecord | null;
  onSubmit: (record: LlmProviderRecord) => Promise<void>;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  trigger?: React.ReactElement | null;
}

type DialogStep = "provider" | "models";

function normalizeModelName(value: string) {
  return value.trim().toLowerCase();
}

function buildModelId(providerId: string, modelName: string) {
  return `${providerId}:${modelName.trim().replace(/[/\s]/g, "-")}`;
}

export function ProviderFormDialog({
  provider,
  onSubmit,
  open: openProp,
  onOpenChange,
  trigger,
}: ProviderFormDialogProps) {
  const [internalOpen, setInternalOpen] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [testingConnection, setTestingConnection] = React.useState(false);
  const [testDialogOpen, setTestDialogOpen] = React.useState(false);
  const [testResult, setTestResult] = React.useState<ProviderTestResult | null>(
    null,
  );
  const [savedProviderId, setSavedProviderId] = React.useState<string | null>(
    provider?.id || null,
  );
  const [step, setStep] = React.useState<DialogStep>(
    provider ? "models" : "provider",
  );
  const [modelList, setModelList] = React.useState<LlmModelRecord[]>([]);
  const [newModelName, setNewModelName] = React.useState("");
  const [addingModel, setAddingModel] = React.useState(false);
  const [modelError, setModelError] = React.useState<string | null>(null);

  const isEditing = !!provider;
  const open = openProp ?? internalOpen;

  const loadPersistedModels = React.useCallback(async (providerId: string) => {
    try {
      const list = await models.listByProvider(providerId);
      setModelList(list);
    } catch (error) {
      console.error("Failed to load models:", error);
      setModelList([]);
    }
  }, []);

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (openProp === undefined) {
        setInternalOpen(nextOpen);
      }
      onOpenChange?.(nextOpen);
      if (!nextOpen) {
        setSavedProviderId(provider?.id || null);
        setStep(provider ? "models" : "provider");
        setNewModelName("");
        setModelError(null);
      }
    },
    [onOpenChange, openProp, provider?.id],
  );

  const [formData, setFormData] = React.useState<LlmProviderRecord>(
    () =>
      provider || {
        id: "",
        kind: "openai-compatible",
        display_name: "",
        base_url: "",
        api_key: "",
        is_default: false,
        extra_headers: {},
        secret_status: "ready",
      },
  );

  // Load models when savedProviderId changes
  React.useEffect(() => {
    if (savedProviderId) {
      void loadPersistedModels(savedProviderId);
    } else {
      setModelList([]);
    }
  }, [savedProviderId, loadPersistedModels]);

  React.useEffect(() => {
    if (provider) {
      setFormData(provider);
      setSavedProviderId(provider.id);
      setStep("models");
    } else {
      setFormData({
        id: "",
        kind: "openai-compatible",
        display_name: "",
        base_url: "",
        api_key: "",
        is_default: false,
        extra_headers: {},
        secret_status: "ready",
      });
      setSavedProviderId(null);
      setStep("provider");
    }
    setTestingConnection(false);
    setTestDialogOpen(false);
    setTestResult(null);
    setNewModelName("");
    setModelError(null);
  }, [provider]);

  const visibleModels = modelList.map((model) => ({
    key: model.id,
    id: model.id,
    name: model.name,
    is_default: model.is_default,
  }));

  const defaultModelName =
    visibleModels.find((model) => model.is_default)?.name ?? "";

  const canSave = Boolean(
    formData.id.trim() &&
    formData.display_name.trim() &&
    formData.base_url.trim() &&
    formData.api_key.trim()
  );

  const handleSaveProvider = async () => {
    if (!canSave) return;

    setSaving(true);
    setModelError(null);
    try {
      await onSubmit(formData);
      setSavedProviderId(formData.id);
      setStep("models");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setModelError(message);
      console.error("Failed to save provider:", error);
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    const record: ProviderInput = { ...formData };
    const modelName = defaultModelName || newModelName.trim();

    if (!modelName) {
      setTestResult({
        provider_id: record.id,
        model: "",
        base_url: record.base_url,
        checked_at: new Date().toISOString(),
        latency_ms: 0,
        status: "model_not_available",
        message: "请先添加一个模型或输入模型名称",
      });
      setTestDialogOpen(true);
      return;
    }

    setTestDialogOpen(true);
    setTestingConnection(true);
    setTestResult(null);
    try {
      const result = await providers.testInput(record, modelName);
      setTestResult(result);
    } catch (error) {
      setTestResult({
        provider_id: record.id,
        model: modelName,
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

  const handleAddModel = async () => {
    const trimmedModelName = newModelName.trim();
    if (!trimmedModelName) return;

    const existingNames = new Set(
      visibleModels.map((model) => normalizeModelName(model.name)),
    );
    if (existingNames.has(normalizeModelName(trimmedModelName))) {
      setModelError(`模型 "${trimmedModelName}" 已存在`);
      return;
    }

    if (!savedProviderId) {
      setModelError("请先保存 Provider");
      return;
    }

    setAddingModel(true);
    setModelError(null);
    try {
      const input: ModelInput = {
        id: buildModelId(savedProviderId, trimmedModelName),
        provider_id: savedProviderId,
        name: trimmedModelName,
        is_default: modelList.length === 0,
      };
      await models.upsert(input);
      setNewModelName("");
      await loadPersistedModels(savedProviderId);
    } catch (error) {
      setModelError(error instanceof Error ? error.message : String(error));
      console.error("Failed to add model:", error);
    } finally {
      setAddingModel(false);
    }
  };

  const handleDeleteModel = async (modelId: string) => {
    try {
      setModelError(null);
      if (!savedProviderId) {
        setModelError("请先保存 Provider");
        return;
      }

      await models.delete(modelId);
      const nextModels = modelList.filter((model) => model.id !== modelId);
      setModelList(nextModels);
      if (nextModels.length > 0 && !nextModels.some((model) => model.is_default)) {
        await handleSetDefaultModel(nextModels[0].id);
      }
    } catch (error) {
      setModelError(error instanceof Error ? error.message : String(error));
      console.error("Failed to delete model:", error);
    }
  };

  const handleSetDefaultModel = async (modelId: string) => {
    try {
      setModelError(null);
      if (!savedProviderId) {
        setModelError("请先保存 Provider");
        return;
      }

      await models.setDefault(modelId);
      setModelList((prev) =>
        prev.map((m) => ({ ...m, is_default: m.id === modelId })),
      );
    } catch (error) {
      setModelError(error instanceof Error ? error.message : String(error));
      console.error("Failed to set default model:", error);
    }
  };

  const canTest = Boolean(
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    (visibleModels.length > 0 || (step === "provider" && newModelName.trim())),
  );

  const defaultTrigger = isEditing ? (
    <Button size="sm" variant="outline">
      <Pencil className="h-3 w-3" />
    </Button>
  ) : (
    <Button size="sm">
      <Plus className="h-4 w-4 mr-1" />
      Add Provider
    </Button>
  );
  const dialogTrigger = trigger === undefined ? defaultTrigger : trigger;
  const draftProvider = {
    id: formData.id,
    kind: formData.kind,
    display_name: formData.display_name || "未命名 Provider",
    base_url: formData.base_url,
    is_default: formData.is_default,
    extra_headers: formData.extra_headers,
    secret_status: formData.secret_status,
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      {dialogTrigger ? <DialogTrigger render={dialogTrigger} /> : null}
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-xl">
        {step === "provider" && (
          <>
            <DialogHeader>
              <DialogTitle>
                {isEditing ? "编辑 Provider" : "添加 Provider"}
              </DialogTitle>
              <DialogDescription>
                {isEditing
                  ? "更新 LLM Provider 配置"
                  : "配置一个新的 LLM Provider"}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-4">
              {formData.secret_status === "requires_reentry" && (
                <div className="rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-sm text-amber-900">
                  当前保存的密钥已无法解密，请重新填写 API Key 后再保存。
                </div>
              )}
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
                <Label htmlFor="display_name">Display Name</Label>
                <Input
                  id="display_name"
                  value={formData.display_name}
                  onChange={(e) =>
                    setFormData({ ...formData, display_name: e.target.value })
                  }
                  placeholder="My LLM Provider"
                  required
                />
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
              <DialogFooter className="gap-2 sm:gap-0">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => onOpenChange?.(false)}
                >
                  取消
                </Button>
                <Button
                  onClick={handleSaveProvider}
                  disabled={saving || !canSave}
                >
                  {saving ? "保存中..." : "下一步: 添加模型"}
                </Button>
              </DialogFooter>
            </div>
          </>
        )}

        {step === "models" && (
          <>
            <DialogHeader>
              <DialogTitle>添加模型</DialogTitle>
              <DialogDescription>
                为 {formData.display_name} 添加模型
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-4">
              <div className="mt-4 border-t pt-4">
          <div className="mb-3 flex items-start justify-between gap-3">
            <div>
              <h4 className="text-sm font-medium">模型列表</h4>
              <p className="text-xs text-muted-foreground">
                一个 Provider 可以配置多个模型，其中一个作为默认模型。
              </p>
            </div>
          </div>

          <div className="mb-3 flex gap-2">
            <Input
              value={newModelName}
              onChange={(e) => setNewModelName(e.target.value)}
              placeholder="gpt-4o, claude-3-opus..."
              className="flex-1"
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void handleAddModel();
                }
              }}
            />
            <Button
              type="button"
              size="sm"
              onClick={() => void handleAddModel()}
              disabled={!newModelName.trim() || addingModel}
            >
              <Plus className="h-4 w-4" />
            </Button>
          </div>

          {modelError ? (
            <div className="mb-3 rounded-md border border-destructive/20 bg-destructive/5 px-3 py-2 text-xs text-destructive">
              {modelError}
            </div>
          ) : null}

          {visibleModels.length > 0 ? (
            <div className="space-y-1">
              {visibleModels.map((model) => (
                <div
                  key={model.key}
                  className="flex items-center justify-between rounded bg-muted/50 p-2 text-sm"
                >
                  <div className="flex items-center gap-2">
                    {model.is_default ? (
                      <Star className="h-3 w-3 fill-yellow-500 text-yellow-500" />
                    ) : null}
                    <span className="font-mono text-xs">{model.name}</span>
                  </div>
                  <div className="flex items-center gap-1">
                    {!model.is_default ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6"
                        onClick={() => void handleSetDefaultModel(model.id)}
                        title="设为默认"
                      >
                        <Star className="h-3 w-3" />
                      </Button>
                    ) : null}
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 text-destructive hover:text-destructive"
                      onClick={() => void handleDeleteModel(model.id)}
                      title="删除"
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">
              添加至少一个模型以便测试连接。
            </p>
          )}

          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              type="button"
              variant="outline"
              onClick={() => setStep("provider")}
            >
              上一步
            </Button>
            <Button onClick={() => onOpenChange?.(false)}>
              完成
            </Button>
          </DialogFooter>
            </div>
            </div>
          </>
        )}

        <ProviderTestDialog
          open={testDialogOpen}
          onOpenChange={setTestDialogOpen}
          provider={draftProvider}
          result={testResult}
          testing={testingConnection}
          onRetest={() => {
            void handleTestConnection();
          }}
        />
      </DialogContent>
    </Dialog>
  );
}

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
