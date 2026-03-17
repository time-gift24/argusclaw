"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { ArrowLeft, Save } from "lucide-react";
import { providers, type ProviderSecretStatus } from "@/lib/tauri";

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
            api_key: provider.api_key,
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
    <div className="w-full px-6 py-6 space-y-4">
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
