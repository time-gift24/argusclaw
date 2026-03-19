"use client"

import * as React from "react"
import { useRouter } from "next/navigation"
import { Save, Plus, X, Check, AlertCircle, Loader2 } from "lucide-react"
import {
  providers,
  type ProviderSecretStatus,
  type ProviderInput,
  type ProviderTestResult,
} from "@/lib/tauri"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Checkbox } from "@/components/ui/checkbox"

export interface LlmProviderRecord {
  id: number
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
  providerId?: number
}

function createDefaultFormData(): LlmProviderRecord {
  return {
    id: 0,
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
  const [newHeaderName, setNewHeaderName] = React.useState("")
  const [newHeaderValue, setNewHeaderValue] = React.useState("")
  const [headersExpanded, setHeadersExpanded] = React.useState(false)

  // Test connection state - one result per model
  const [testResults, setTestResults] = React.useState<Record<string, ProviderTestResult>>({})
  const [testingModels, setTestingModels] = React.useState<Set<string>>(new Set())

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
        secret_status: formData.secret_status,
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

  const handleAddHeader = () => {
    const name = newHeaderName.trim()
    const value = newHeaderValue.trim()
    if (!name) return
    if (formData.extra_headers[name] !== undefined) return
    setFormData({
      ...formData,
      extra_headers: { ...formData.extra_headers, [name]: value },
    })
    setNewHeaderName("")
    setNewHeaderValue("")
  }

  const handleRemoveHeader = (name: string) => {
    setFormData((prev) => {
      const next = { ...prev.extra_headers }
      delete next[name]
      return { ...prev, extra_headers: next }
    })
  }

  const handleAddModel = async () => {
    const model = newModel.trim()
    if (!model || formData.models.includes(model)) return

    const newModels = [...formData.models, model]
    setFormData({
      ...formData,
      models: newModels,
      default_model: formData.default_model || model,
    })
    setNewModel("")

    // Auto-test the new model if we have all required fields
    if (formData.id > 0 && formData.base_url.trim() && formData.api_key.trim()) {
      await testModel(model, newModels)
    }
  }

  const testModel = async (model: string, models: string[]) => {
    setTestingModels((prev) => new Set(prev).add(model))

    try {
      const input: ProviderInput = {
        id: formData.id,
        kind: formData.kind,
        display_name: formData.display_name,
        base_url: formData.base_url,
        api_key: formData.api_key,
        models: models,
        default_model: formData.default_model || model,
        is_default: formData.is_default,
        extra_headers: formData.extra_headers,
        secret_status: formData.secret_status,
      }

      // For existing providers, test the saved record
      // For new providers, test the input without saving
      const result = formData.id > 0
        ? await providers.testConnection(formData.id, model)
        : await providers.testInput(input, model)

      setTestResults((prev) => ({ ...prev, [model]: result }))
    } catch (error) {
      const fallbackResult: ProviderTestResult = {
        provider_id: String(formData.id),
        model,
        base_url: formData.base_url,
        checked_at: new Date().toISOString(),
        latency_ms: 0,
        status: "request_failed",
        message: error instanceof Error ? error.message : String(error),
        request: undefined,
        response: undefined,
      }
      setTestResults((prev) => ({ ...prev, [model]: fallbackResult }))
      console.error("Failed to test model:", error)
    } finally {
      setTestingModels((prev) => {
        const next = new Set(prev)
        next.delete(model)
        return next
      })
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
    setTestResults((prev) => {
      const next = { ...prev }
      delete next[model]
      return next
    })
  }

  const handleRetestModel = async (model: string) => {
    await testModel(model, formData.models)
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
        <Button size="sm" onClick={handleSubmit} disabled={saving || !canSave}>
          <Save className="h-4 w-4 mr-1" />
          {saving ? "保存中..." : "保存"}
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-6">
        {/* Left: Basic Info + Models */}
        <div className="space-y-4">
          {/* Basic Info */}
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
            <Checkbox
              id="is_default"
              checked={formData.is_default}
              onCheckedChange={(checked) => setFormData({ ...formData, is_default: !!checked })}
            />
            <Label htmlFor="is_default" className="cursor-pointer">
              设为默认提供者
            </Label>
          </div>

          {/* Extra Headers Section */}
          <div className="space-y-2 pt-4 border-t">
            <button
              type="button"
              className="flex items-center gap-2 text-sm font-medium w-full"
              onClick={() => setHeadersExpanded((v) => !v)}
            >
              <span className="text-xs text-muted-foreground">▸</span>
              Extra Headers
            </button>
            {headersExpanded && (
              <div className="space-y-2 pl-3">
                <div className="flex gap-2">
                  <Input
                    value={newHeaderName}
                    onChange={(e) => setNewHeaderName(e.target.value)}
                    placeholder="Header 名称"
                    className="text-sm"
                  />
                  <Input
                    value={newHeaderValue}
                    onChange={(e) => setNewHeaderValue(e.target.value)}
                    placeholder="Header 值"
                    className="text-sm"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void handleAddHeader()}
                    disabled={!newHeaderName.trim()}
                  >
                    <Plus className="h-4 w-4" />
                  </Button>
                </div>
                {Object.keys(formData.extra_headers).length > 0 && (
                  <div className="space-y-1">
                    {Object.entries(formData.extra_headers).map(([name, value]) => (
                      <div key={name} className="flex items-center gap-2 text-xs">
                        <Badge variant="secondary" className="shrink-0 font-mono">
                          {name}
                        </Badge>
                        <span className="truncate text-muted-foreground font-mono flex-1 min-w-0">
                          {value || <span className="italic opacity-40">(空)</span>}
                        </span>
                        <button
                          type="button"
                          className="shrink-0 hover:text-destructive"
                          onClick={() => handleRemoveHeader(name)}
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
                <p className="text-[11px] text-muted-foreground">
                  用于向 API 请求添加自定义 HTTP Header
                </p>
              </div>
            )}
          </div>

          {/* Models Section */}
          <div className="space-y-2 pt-4 border-t">
            <Label>模型列表</Label>
            <div className="flex gap-2">
              <Input
                value={newModel}
                onChange={(e) => setNewModel(e.target.value)}
                placeholder="输入模型名称后回车添加"
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault()
                    void handleAddModel()
                  }
                }}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void handleAddModel()}
                disabled={!newModel.trim()}
              >
                <Plus className="h-4 w-4" />
              </Button>
            </div>
            {formData.models.length > 0 && (
              <div className="flex flex-wrap gap-2 mt-2">
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
            )}
            <p className="text-[11px] text-muted-foreground">
              点击标签设为默认模型，添加模型后自动测试连接
            </p>
          </div>
        </div>

        {/* Right: Test Results */}
        <div className="space-y-2">
          <Label>连接测试</Label>
          <div className="border rounded-lg divide-y min-h-[300px]">
            {formData.models.length === 0 ? (
              <div className="flex items-center justify-center h-full text-muted-foreground text-sm py-12">
                添加模型后将自动测试连接
              </div>
            ) : (
              formData.models.map((model) => {
                const result = testResults[model]
                const isTesting = testingModels.has(model)
                const isSuccess = result?.status === "success"
                const isFailed = result && result.status !== "success"
                const hasDetails = result && (result.request != null || result.response != null)

                return (
                  <Collapsible key={model} defaultOpen={false} className="w-full">
                    <div className="flex items-center justify-between px-3 py-2">
                      <div className="flex items-center gap-2 min-w-0">
                        <Badge
                          variant={model === formData.default_model ? "default" : "secondary"}
                          className="shrink-0"
                        >
                          {model}
                        </Badge>
                        {isTesting && (
                          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                        )}
                        {isSuccess && (
                          <Check className="h-4 w-4 text-green-500" />
                        )}
                        {isFailed && (
                          <AlertCircle className="h-4 w-4 text-destructive" />
                        )}
                        {result && !isTesting && (
                          <span className="text-xs text-muted-foreground">
                            {result.latency_ms}ms
                          </span>
                        )}
                      </div>
                      <div className="flex items-center gap-1 shrink-0">
                        {hasDetails && (
                          <CollapsibleTrigger className="h-7 px-2 text-xs text-muted-foreground hover:text-foreground hover:bg-muted/50 rounded border-0 cursor-pointer bg-transparent">
                            详情
                          </CollapsibleTrigger>
                        )}
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-7"
                          onClick={() => void handleRetestModel(model)}
                          disabled={isTesting || !formData.api_key.trim()}
                        >
                          {isTesting ? "测试中..." : "重测"}
                        </Button>
                      </div>
                    </div>
                    <CollapsibleContent>
                      <div className="px-3 pb-3 pt-1 border-t space-y-2">
                        {result?.request != null && (
                          <div>
                            <p className="text-[10px] font-medium text-muted-foreground mb-1">
                              请求
                            </p>
                            <pre className="overflow-x-auto rounded bg-muted/30 p-2 font-mono text-[10px] leading-relaxed whitespace-pre-wrap break-all">
                              {(() => {
                                try {
                                  return JSON.stringify(JSON.parse(result!.request!), null, 2)
                                } catch {
                                  return result!.request
                                }
                              })()}
                            </pre>
                          </div>
                        )}
                        {result?.response != null && isSuccess && (
                          <div>
                            <p className="text-[10px] font-medium text-muted-foreground mb-1">
                              响应
                            </p>
                            <pre className="overflow-x-auto rounded bg-muted/30 p-2 font-mono text-[10px] leading-relaxed whitespace-pre-wrap break-all">
                              {result!.response}
                            </pre>
                          </div>
                        )}
                      </div>
                    </CollapsibleContent>
                  </Collapsible>
                )
              })
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
