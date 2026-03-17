"use client";

import * as React from "react";
import { Plus, Pencil } from "lucide-react";
import {
  providers,
  type ProviderSecretStatus,
  type ProviderInput,
  type ProviderTestResult,
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
  models: string[];
  default_model: string;
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
  const isEditing = !!provider;
  const open = openProp ?? internalOpen;

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (openProp === undefined) {
        setInternalOpen(nextOpen);
      }
      onOpenChange?.(nextOpen);
    },
    [onOpenChange, openProp],
  );

  const [formData, setFormData] = React.useState<LlmProviderRecord>(
    () =>
      provider || {
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
      },
  );

  React.useEffect(() => {
    if (provider) {
      setFormData(provider);
    } else {
      setFormData({
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
      });
    }
    setTestingConnection(false);
    setTestDialogOpen(false);
    setTestResult(null);
  }, [provider]);

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
    formData.default_model.trim(),
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
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      {dialogTrigger ? <DialogTrigger render={dialogTrigger} /> : null}
      <DialogContent className="sm:max-w-md">
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
          <div className="space-y-2">
            <Label htmlFor="models">Models (comma-separated)</Label>
            <Input
              id="models"
              value={formData.models.join(", ")}
              onChange={(e) =>
                setFormData({
                  ...formData,
                  models: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
                })
              }
              placeholder="gpt-4, gpt-4o, gpt-3.5-turbo"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="default_model">Default Model</Label>
            <Input
              id="default_model"
              value={formData.default_model}
              onChange={(e) =>
                setFormData({ ...formData, default_model: e.target.value })
              }
              placeholder="gpt-4"
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
