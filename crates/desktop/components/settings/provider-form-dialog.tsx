"use client";

import * as React from "react";
import { Plus, Pencil } from "lucide-react";
import {
  providers,
  type ProviderSecretStatus,
  type ProviderInput,
  type ProviderTestResult,
  type ModelConfig,
} from "@/lib/tauri";
import { Badge } from "@/components/ui/badge";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ProviderTestDialog } from "./provider-test-dialog";

export interface LlmProviderRecord {
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
  model_config: Record<string, ModelConfig>;
}

interface ProviderFormDialogProps {
  provider?: LlmProviderRecord | null;
  onSubmit: (record: LlmProviderRecord) => Promise<void>;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  trigger?: React.ReactElement | null;
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
    is_default: false,
    extra_headers: {},
    secret_status: "ready",
    model_config: {},
  };
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
  const [newModel, setNewModel] = React.useState("");
  const isEditing = !!provider;
  const open = openProp ?? internalOpen;

  const [formData, setFormData] = React.useState<LlmProviderRecord>(
    () => provider || createDefaultFormData(),
  );

  React.useEffect(() => {
    if (provider) {
      setFormData(provider);
    } else {
      setFormData(createDefaultFormData());
    }
    setTestingConnection(false);
    setTestDialogOpen(false);
    setTestResult(null);
    setNewModel("");
  }, [provider]);

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (openProp === undefined) {
        setInternalOpen(nextOpen);
      }
      onOpenChange?.(nextOpen);
    },
    [onOpenChange, openProp],
  );

  const handleAddModel = React.useCallback(() => {
    const trimmed = newModel.trim();
    if (!trimmed) return;
    setFormData((prev) => {
      if (prev.models.includes(trimmed)) {
        return prev;
      }
      const newModels = [...prev.models, trimmed];
      return {
        ...prev,
        models: newModels,
        default_model: prev.default_model || trimmed,
      };
    });
    setNewModel("");
  }, [newModel]);

  const handleRemoveModel = React.useCallback((model: string) => {
    setFormData((prev) => {
      const newModels = prev.models.filter((m) => m !== model);
      return {
        ...prev,
        models: newModels,
        default_model: newModels.includes(prev.default_model)
          ? prev.default_model
          : newModels[0] || "",
      };
    });
  }, []);

  const handleSetDefaultModel = React.useCallback((model: string) => {
    setFormData((prev) => ({ ...prev, default_model: model }));
  }, []);

  const handleUpdateModelContextLength = React.useCallback(
    (model: string, contextLength: number | null) => {
      setFormData((prev) => {
        const newModelConfig = { ...prev.model_config };
        if (contextLength === null) {
          // If context_length is cleared, remove it from config
          const existingConfig = newModelConfig[model];
          if (existingConfig) {
            const { context_length: _, ...rest } = existingConfig;
            if (Object.keys(rest).length === 0) {
              delete newModelConfig[model];
            } else {
              newModelConfig[model] = rest as ModelConfig;
            }
          }
        } else {
          newModelConfig[model] = {
            ...newModelConfig[model],
            context_length: contextLength,
          };
        }
        return { ...prev, model_config: newModelConfig };
      });
    },
    [],
  );

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    try {
      await onSubmit(formData);
      handleOpenChange(false);
    } catch (error) {
      console.error("Failed to save provider:", error);
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    const record: ProviderInput = { ...formData };
    setTestDialogOpen(true);
    setTestingConnection(true);
    setTestResult(null);
    try {
      const result = await providers.testInput(record, record.default_model);
      setTestResult(result);
    } catch (error) {
      setTestResult({
        provider_id: record.id,
        model: record.default_model,
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

  const canTest = Boolean(
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    formData.models.length > 0,
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
    models: formData.models,
    default_model: formData.default_model,
    is_default: formData.is_default,
    extra_headers: formData.extra_headers,
    secret_status: formData.secret_status,
    model_config: formData.model_config,
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      {dialogTrigger ? <DialogTrigger render={dialogTrigger} /> : null}
      <DialogContent className="sm:max-w-2xl">
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

          {/* Two-column grid layout */}
          <div className="grid grid-cols-1 gap-6 sm:grid-cols-2">
            {/* Left Column: Basic Info */}
            <div className="space-y-4">
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
                <Label htmlFor="kind">Provider Type</Label>
                <Select
                  value={formData.kind}
                  onValueChange={(value) =>
                    setFormData({ ...formData, kind: value as "openai-compatible" })
                  }
                  disabled={isEditing}
                >
                  <SelectTrigger id="kind">
                    <SelectValue placeholder="Select provider type" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="openai-compatible">OpenAI Compatible</SelectItem>
                  </SelectContent>
                </Select>
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
            </div>

            {/* Right Column: Model Configuration */}
            <div className="space-y-4">
              <div className="space-y-2">
                <Label>Models</Label>
                <div className="flex flex-wrap gap-2 mb-2">
                  {formData.models.map((model) => (
                    <Badge
                      key={model}
                      variant={model === formData.default_model ? "default" : "secondary"}
                      className="cursor-pointer pr-1"
                      onClick={() => handleSetDefaultModel(model)}
                    >
                      {model}
                      {model === formData.default_model && (
                        <span className="ml-1 text-[10px] opacity-70">默认</span>
                      )}
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
                    </Badge>
                  ))}
                </div>
                <div className="flex gap-2">
                  <Input
                    value={newModel}
                    onChange={(e) => setNewModel(e.target.value)}
                    placeholder="输入模型名称"
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
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
                  点击标签设为默认模型
                </p>
                {formData.models.length > 0 && (
                  <div className="mt-3 space-y-2">
                    <Label className="text-xs text-muted-foreground">
                      模型上下文长度配置（可选）
                    </Label>
                    <div className="space-y-1">
                      {formData.models.map((model) => (
                        <div
                          key={model}
                          className="flex items-center gap-2 text-sm"
                        >
                          <span className="w-32 truncate text-xs" title={model}>
                            {model}
                          </span>
                          <Input
                            type="number"
                            placeholder="默认 128000"
                            value={
                              formData.model_config[model]?.context_length ?? ""
                            }
                            onChange={(e) => {
                              const value = e.target.value;
                              if (value === "") {
                                handleUpdateModelContextLength(model, null);
                              } else {
                                const num = parseInt(value, 10);
                                if (!isNaN(num) && num > 0) {
                                  handleUpdateModelContextLength(model, num);
                                }
                              }
                            }}
                            className="h-7 w-28 text-xs"
                          />
                          <span className="text-[10px] text-muted-foreground">
                            tokens
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </div>
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
