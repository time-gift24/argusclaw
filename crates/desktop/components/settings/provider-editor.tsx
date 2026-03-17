"use client"

import * as React from "react"
import { useRouter } from "next/navigation"
import { Save, Plus, X } from "lucide-react"
import {
  providers,
  type ProviderSecretStatus,
  type ProviderInput,
  type ProviderTestResult,
} from "@/lib/tauri"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ProviderTestDialog } from "./provider-test-dialog"

export interface LlmProviderRecord {
  id: string
  kind: "openai-compatible"
  display_name: string
  base_url: string
  api_key: string
  models: string[]
  default_model: string
  is_default: boolean
  extra_headers: Record<string, string>
  secret_status: ProviderSecretStatus
}

interface ProviderEditorProps {
  providerId?: string
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
  }
}

export function ProviderEditor({ providerId }: ProviderEditorProps) {
  const router = useRouter()
  const isEditing = !!providerId

  const [loading, setLoading] = React.useState(isEditing)
  const [saving, setSaving] = React.useState(false)
  const [formData, setFormData] = React.useState<LlmProviderRecord>(createDefaultFormData)
  const [newModel, setNewModel] = React.useState("")

  // Test connection state
  const [testDialogOpen, setTestDialogOpen] = React.useState(false)
  const [testResult, setTestResult] = React.useState<ProviderTestResult | null>(null)
  const [testingConnection, setTestingConnection] = React.useState(false)

  // Load provider data if editing
  React.useEffect(() => {
    const loadData = async () => {
      if (providerId) {
        try {
          const provider = await providers.get(providerId)
          if (provider) {
            setFormData({
              id: provider.id,
              kind: provider.kind,
              display_name: provider.display_name,
              base_url: provider.base_url,
              api_key:
                typeof provider.api_key === "string"
                  ? provider.api_key
                  : (provider.api_key as { api_key: string }).api_key || "",
              models: provider.models,
              default_model: provider.default_model,
              is_default: provider.is_default,
              extra_headers: provider.extra_headers,
              secret_status: provider.secret_status,
            })
          }
        } catch (error) {
          console.error("Failed to load provider:", error)
        } finally {
          setLoading(false)
        }
      } else {
        setLoading(false)
      }
    }
    loadData()
  }, [providerId])

  const canSave = Boolean(
    formData.id.trim() &&
    formData.display_name.trim() &&
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    formData.models.length > 0,
  )

  const handleSubmit = async () => {
    if (!canSave) return

    setSaving(true)
    try {
      const input: ProviderInput = {
        id: formData.id,
        kind: formData.kind,
        display_name: formData.display_name,
        base_url: formData.base_url,
        api_key: formData.api_key,
        models: formData.models,
        default_model: formData.default_model,
        is_default: formData.is_default,
        extra_headers: formData.extra_headers,
      }
      await providers.upsert(input)
      router.push("/settings/providers")
    } catch (error) {
      console.error("Failed to save provider:", error)
    } finally {
      setSaving(false)
    }
  }

  const handleSetDefaultModel = (model: string) => {
    setFormData({ ...formData, default_model: model })
  }

  const handleAddModel = () => {
    if (newModel.trim() && !formData.models.includes(newModel.trim())) {
      setFormData({
        ...formData,
        models: [...formData.models, newModel.trim()],
      })
      setNewModel("")
    }
  }

  const handleRemoveModel = (model: string) => {
    setFormData((prev) => {
      const newModels = prev.models.filter((m) => m !== model)
      return {
        ...prev,
        models: newModels,
        default_model: prev.default_model === model ? (newModels[0] || "") : prev.default_model,
      }
    })
  }

  const handleTestConnection = async () => {
    if (!formData.default_model) return

    setTestingConnection(true)
    try {
      // Save first to ensure provider exists
      const input: ProviderInput = {
        id: formData.id,
        kind: formData.kind,
        display_name: formData.display_name,
        base_url: formData.base_url,
        api_key: formData.api_key,
        models: formData.models,
        default_model: formData.default_model,
        is_default: formData.is_default,
        extra_headers: formData.extra_headers,
      }
      await providers.upsert(input)

      const result = await providers.testConnection(formData.id, formData.default_model)
      setTestResult(result)
      setTestDialogOpen(true)
    } catch (error) {
      setTestResult({
        provider_id: formData.id,
        model: formData.default_model,
        base_url: formData.base_url,
        checked_at: new Date().toISOString(),
        latency_ms: 0,
        status: "request_failed",
        message: error instanceof Error ? error.message : String(error),
      })
      setTestDialogOpen(true)
      console.error("Failed to test provider:", error)
    } finally {
      setTestingConnection(false)
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-sm font-semibold">
          {isEditing ? "编辑 LLM 提供者" : "新建 LLM 提供者"}
        </h1>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={handleTestConnection}
            disabled={saving || testingConnection || !canSave}
          >
            {testingConnection ? "测试中..." : "测试连接"}
          </Button>
          <Button size="sm" onClick={handleSubmit} disabled={saving || !canSave}>
            <Save className="h-4 w-4 mr-1" />
            {saving ? "保存中..." : "保存"}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-6">
        {/* Left: Basic Info */}
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="id">ID</Label>
              <Input
                id="id"
                value={formData.id}
                onChange={(e) => setFormData({ ...formData, id: e.target.value })}
                placeholder="my-provider"
                required
                disabled={isEditing}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="display_name">名称</Label>
              <Input
                id="display_name"
                value={formData.display_name}
                onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
                placeholder="我的提供者"
                required
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="base_url">Base URL</Label>
            <Input
              id="base_url"
              value={formData.base_url}
              onChange={(e) => setFormData({ ...formData, base_url: e.target.value })}
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
              onChange={(e) => setFormData({ ...formData, api_key: e.target.value })}
              placeholder="sk-..."
              required
            />
          </div>

          <div className="flex items-center gap-2">
            <input
              id="is_default"
              type="checkbox"
              checked={formData.is_default}
              onChange={(e) => setFormData({ ...formData, is_default: e.target.checked })}
              className="h-4 w-4"
            />
            <Label htmlFor="is_default" className="cursor-pointer">
              设为默认提供者
            </Label>
          </div>
        </div>

        {/* Right: Models */}
        <div className="space-y-4">
          <div className="space-y-2">
            <Label>模型列表</Label>
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
                      e.stopPropagation()
                      handleRemoveModel(model)
                    }}
                  >
                    <X className="h-3 w-3" />
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
                    e.preventDefault()
                    handleAddModel()
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
                <Plus className="h-4 w-4" />
                添加
              </Button>
            </div>
            <p className="text-[11px] text-muted-foreground">
              点击标签设为默认模型
            </p>
          </div>
        </div>
      </div>

      <ProviderTestDialog
        open={testDialogOpen}
        onOpenChange={setTestDialogOpen}
        provider={formData}
        result={testResult}
        testing={testingConnection}
        onRetest={() => void handleTestConnection()}
      />
    </div>
  )
}
