"use client";

import * as React from "react";
import { Plus, Pencil, Trash2, Star } from "lucide-react";
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
  const [newModelName, setNewModelName] = React.useState("");
  const [addingModel, setAddingModel] = React.useState(false);

  const isEditing = !!provider;
  const open = openProp ?? internalOpen;

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (openProp === undefined) {
        setInternalOpen(nextOpen);
      }
      onOpenChange?.(nextOpen);
      if (!nextOpen) {
        setSavedProviderId(provider?.id || null);
        setNewModelName("");
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
        try {
          const list = await models.listByProvider(targetId);
          setModelList(list);
        } catch (error) {
          console.error("Failed to load models:", error);
          setModelList([]);
        }
      } else {
        setModelList([]);
      }
    };
    void loadModels();
  }, [savedProviderId, provider?.id]);

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
    setTestingConnection(false);
    setTestDialogOpen(false);
    setTestResult(null);
    setNewModelName("");
  }, [provider]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    try {
      await onSubmit(formData);
      setSavedProviderId(formData.id);
    } catch (error) {
      console.error("Failed to save provider:", error);
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    const record: ProviderInput = { ...formData };
    // Use the default model or first model from the list
    const defaultModel = modelList.find((m) => m.is_default) || modelList[0];
    const modelName = defaultModel?.name || newModelName.trim();

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
    if (!newModelName.trim() || !savedProviderId) return;

    setAddingModel(true);
    try {
      const isFirst = modelList.length === 0;
      const modelId = `${savedProviderId}:${newModelName.trim().replace(/[/\s]/g, "-")}`;
      const input: ModelInput = {
        id: modelId,
        provider_id: savedProviderId,
        name: newModelName.trim(),
        is_default: isFirst,
      };
      await models.upsert(input);
      setNewModelName("");
      // Reload models
      const list = await models.listByProvider(savedProviderId);
      setModelList(list);
    } catch (error) {
      console.error("Failed to add model:", error);
    } finally {
      setAddingModel(false);
    }
  };

  const handleDeleteModel = async (modelId: string) => {
    try {
      await models.delete(modelId);
      setModelList((prev) => prev.filter((m) => m.id !== modelId));
    } catch (error) {
      console.error("Failed to delete model:", error);
    }
  };

  const handleSetDefaultModel = async (modelId: string) => {
    try {
      await models.setDefault(modelId);
      setModelList((prev) =>
        prev.map((m) => ({ ...m, is_default: m.id === modelId })),
      );
    } catch (error) {
      console.error("Failed to set default model:", error);
    }
  };

  const canTest = Boolean(
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    (modelList.length > 0 || newModelName.trim()),
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
      <DialogContent className="sm:max-w-lg">
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

        {/* Model Management Section - shown after provider is saved */}
        {savedProviderId && (
          <div className="border-t pt-4 mt-4">
            <h4 className="text-sm font-medium mb-3">模型列表</h4>
            <div className="flex gap-2 mb-3">
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
            {modelList.length > 0 ? (
              <div className="space-y-1 max-h-40 overflow-y-auto">
                {modelList.map((model) => (
                  <div
                    key={model.id}
                    className="flex items-center justify-between p-2 rounded bg-muted/50 text-sm"
                  >
                    <div className="flex items-center gap-2">
                      {model.is_default && (
                        <Star className="h-3 w-3 text-yellow-500 fill-yellow-500" />
                      )}
                      <span className="font-mono text-xs">{model.name}</span>
                    </div>
                    <div className="flex items-center gap-1">
                      {!model.is_default && (
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
                      )}
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
                添加模型后才能使用此 Provider
              </p>
            )}
          </div>
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
