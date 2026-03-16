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

interface DraftModel {
  tempId: string;
  name: string;
  is_default: boolean;
}

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
  const [modelList, setModelList] = React.useState<LlmModelRecord[]>([]);
  const [draftModels, setDraftModels] = React.useState<DraftModel[]>([]);
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
        setDraftModels([]);
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

  // Load models when provider changes
  React.useEffect(() => {
    const loadModels = async () => {
      const targetId = savedProviderId || provider?.id;
      if (targetId) {
        await loadPersistedModels(targetId);
      } else {
        setModelList([]);
      }
    };
    void loadModels();
  }, [loadPersistedModels, savedProviderId, provider?.id]);

  React.useEffect(() => {
    if (provider) {
      setFormData(provider);
      setSavedProviderId(provider.id);
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
    }
    setDraftModels([]);
    setTestingConnection(false);
    setTestDialogOpen(false);
    setTestResult(null);
    setNewModelName("");
    setModelError(null);
  }, [provider]);

  const visibleModels = savedProviderId
    ? modelList.map((model) => ({
        key: model.id,
        id: model.id,
        name: model.name,
        is_default: model.is_default,
        persisted: true,
      }))
    : draftModels.map((model) => ({
        key: model.tempId,
        id: model.tempId,
        name: model.name,
        is_default: model.is_default,
        persisted: false,
      }));

  const defaultModelName =
    visibleModels.find((model) => model.is_default)?.name ?? "";

  const persistDraftModels = React.useCallback(
    async (providerId: string) => {
      for (const model of draftModels) {
        const input: ModelInput = {
          id: buildModelId(providerId, model.name),
          provider_id: providerId,
          name: model.name,
          is_default: model.is_default,
        };
        await models.upsert(input);
      }
      setDraftModels([]);
      await loadPersistedModels(providerId);
    },
    [draftModels, loadPersistedModels],
  );

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setModelError(null);
    try {
      await onSubmit(formData);
      setSavedProviderId(formData.id);
      if (draftModels.length > 0) {
        await persistDraftModels(formData.id);
      } else {
        await loadPersistedModels(formData.id);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setModelError(message);
      console.error("Failed to save provider or models:", error);
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

    setAddingModel(true);
    setModelError(null);
    try {
      if (!savedProviderId) {
        const nextDraftModel: DraftModel = {
          tempId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
          name: trimmedModelName,
          is_default: draftModels.length === 0,
        };
        setDraftModels((current) => [...current, nextDraftModel]);
        setNewModelName("");
        return;
      }

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
        setDraftModels((current) => {
          const next = current.filter((model) => model.tempId !== modelId);
          if (next.length > 0 && !next.some((model) => model.is_default)) {
            next[0] = { ...next[0], is_default: true };
          }
          return next;
        });
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
        setDraftModels((current) =>
          current.map((model) => ({
            ...model,
            is_default: model.tempId === modelId,
          })),
        );
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
    (visibleModels.length > 0 || newModelName.trim()),
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
        <DialogHeader>
          <DialogTitle>
            {isEditing ? "Edit Provider" : "Add Provider"}
          </DialogTitle>
          <DialogDescription>
            {isEditing
              ? "Update the LLM provider configuration."
              : "Configure a new LLM provider."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
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
              onClick={handleTestConnection}
              disabled={saving || testingConnection || !canTest}
            >
              {testingConnection ? "正在测试" : "测试连接"}
            </Button>
            <Button type="submit" disabled={saving || testingConnection}>
              {saving ? "Saving..." : isEditing ? "Update" : "Create"}
            </Button>
          </DialogFooter>
        </form>

        <div className="mt-4 border-t pt-4">
          <div className="mb-3 flex items-start justify-between gap-3">
            <div>
              <h4 className="text-sm font-medium">模型列表</h4>
              <p className="text-xs text-muted-foreground">
                一个 Provider 可以配置多个模型，其中一个作为默认模型。
              </p>
            </div>
            {!savedProviderId ? (
              <span className="rounded-full border border-dashed px-2 py-1 text-[11px] text-muted-foreground">
                保存 Provider 后写入
              </span>
            ) : null}
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
                    {!model.persisted ? (
                      <span className="rounded-full bg-background px-1.5 py-0.5 text-[10px] text-muted-foreground">
                        草稿
                      </span>
                    ) : null}
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
              先添加至少一个模型。新建 Provider 时，这些模型会在保存后一起创建。
            </p>
          )}
        </div>

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
