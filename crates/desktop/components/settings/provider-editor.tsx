"use client"

import * as React from "react"
import { useRouter } from "next/navigation"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  SaveIcon,
  Add01Icon,
  Cancel01Icon,
  CheckmarkCircle02Icon,
  AlertCircleIcon,
  Loading03Icon,
  CloudIcon,
  Settings02Icon,
  KeyIcon,
  CodeIcon,
  LinkIcon,
  ArrowDown02Icon,
} from "@hugeicons/core-free-icons"
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
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  Field,
  FieldGroup,
  FieldDescription,
  FieldTitle,
} from "@/components/ui/field"
import { useToast } from "@/components/ui/toast"

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
  const { addToast } = useToast()
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
  const [hasAutoTested, setHasAutoTested] = React.useState(false)

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

  // Auto-test all models when provider is loaded and has models
  React.useEffect(() => {
    if (!loading && isEditing && formData.models.length > 0 && !hasAutoTested) {
      setHasAutoTested(true)
      // Test all models after a short delay to let UI render
      const timer = setTimeout(() => {
        formData.models.forEach((model) => {
          testModel(model, formData.models)
        })
      }, 500)
      return () => clearTimeout(timer)
    }
  }, [loading, isEditing, formData.models, hasAutoTested])

  const canSave = Boolean(
    formData.display_name.trim() &&
    formData.base_url.trim() &&
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
      addToast("success", isEditing ? "LLM 提供者已更新" : "LLM 提供者已创建")
      router.push("/settings/providers")
    } catch (error) {
      console.error("Failed to save provider:", error)
      addToast("error", "保存失败，请重试")
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
    <div className="w-full space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-base font-semibold">
            {isEditing ? "编辑 LLM 提供者" : "新建 LLM 提供者"}
          </h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {isEditing ? `ID: ${providerId}` : "配置新的 LLM 服务连接"}
          </p>
        </div>
        <Button size="sm" onClick={handleSubmit} disabled={saving || !canSave}>
          {saving ? (
            <>
              <HugeiconsIcon icon={ArrowDown02Icon} strokeWidth={2} className="size-3.5 animate-spin" />
              保存中...
            </>
          ) : (
            <>
              <HugeiconsIcon icon={SaveIcon} strokeWidth={2} className="size-3.5" />
              保存
            </>
          )}
        </Button>
      </div>

      {/* Main Content */}
      <div className="grid grid-cols-5 gap-6">
        {/* Left - Basic Info + Models */}
        <div className="col-span-2 space-y-4">
          {/* Basic Info */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <HugeiconsIcon icon={CloudIcon} strokeWidth={2} className="size-4 text-muted-foreground" />
                <CardTitle>基本信息</CardTitle>
              </div>
              <CardDescription>配置 LLM 服务的基本连接信息</CardDescription>
            </CardHeader>
            <CardContent>
              <FieldGroup className="gap-4">
                <Field>
                  <FieldTitle>
                    名称 <span className="text-destructive">*</span>
                  </FieldTitle>
                  <Input
                    id="display_name"
                    value={formData.display_name}
                    onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
                    placeholder="我的提供者"
                    required
                  />
                </Field>

                <Field>
                  <FieldTitle>
                    <div className="flex items-center gap-1">
                      Base URL <span className="text-destructive">*</span>
                    </div>
                  </FieldTitle>
                  <Input
                    id="base_url"
                    value={formData.base_url}
                    onChange={(e) => setFormData({ ...formData, base_url: e.target.value })}
                    placeholder="https://api.example.com/v1"
                    required
                  />
                </Field>

                <Field>
                  <FieldTitle>
                    <div className="flex items-center gap-1">
                      <HugeiconsIcon icon={KeyIcon} strokeWidth={2} className="size-3" />
                      API Key
                    </div>
                  </FieldTitle>
                  <Input
                    id="api_key"
                    type="password"
                    value={formData.api_key}
                    onChange={(e) => setFormData({ ...formData, api_key: e.target.value })}
                    placeholder="sk-... (可选，登录后配置)"
                  />
                  <FieldDescription>API Key 是可选的，稍后可以在设置中配置</FieldDescription>
                </Field>

                <Field orientation="horizontal">
                  <Checkbox
                    id="is_default"
                    checked={formData.is_default}
                    onCheckedChange={(checked) => setFormData({ ...formData, is_default: !!checked })}
                  />
                  <FieldContent>
                    <FieldTitle className="cursor-pointer">设为默认提供者</FieldTitle>
                  </FieldContent>
                </Field>
              </FieldGroup>
            </CardContent>
          </Card>

          {/* Extra Headers */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <HugeiconsIcon icon={CodeIcon} strokeWidth={2} className="size-4 text-muted-foreground" />
                  <CardTitle>Extra Headers</CardTitle>
                </div>
                {Object.keys(formData.extra_headers).length > 0 && (
                  <Badge variant="secondary" className="text-[10px]">
                    {Object.keys(formData.extra_headers).length}
                  </Badge>
                )}
              </div>
              <CardDescription>自定义 HTTP Header</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-3">
                <div className="flex gap-2">
                  <Input
                    value={newHeaderName}
                    onChange={(e) => setNewHeaderName(e.target.value)}
                    placeholder="Header 名称"
                    className="flex-1"
                  />
                  <Input
                    value={newHeaderValue}
                    onChange={(e) => setNewHeaderValue(e.target.value)}
                    placeholder="Header 值"
                    className="flex-1"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void handleAddHeader()}
                    disabled={!newHeaderName.trim()}
                  >
                    <HugeiconsIcon icon={Add01Icon} strokeWidth={2} className="size-3.5" />
                  </Button>
                </div>

                {Object.keys(formData.extra_headers).length > 0 && (
                  <div className="space-y-1.5">
                    {Object.entries(formData.extra_headers).map(([name, value]) => (
                      <div
                        key={name}
                        className="flex items-center gap-2 px-2 py-1.5 rounded-md bg-muted/50 text-xs"
                      >
                        <Badge variant="secondary" className="shrink-0 font-mono text-[10px]">
                          {name}
                        </Badge>
                        <span className="truncate text-muted-foreground font-mono flex-1 min-w-0">
                          {value || <span className="italic opacity-40">(空)</span>}
                        </span>
                        <button
                          type="button"
                          className="shrink-0 hover:text-destructive transition-colors cursor-pointer"
                          onClick={() => handleRemoveHeader(name)}
                        >
                          <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} className="size-3" />
                        </button>
                      </div>
                    ))}
                  </div>
                )}

                <p className="text-[11px] text-muted-foreground">
                  用于向 API 请求添加自定义 HTTP Header
                </p>
              </div>
            </CardContent>
          </Card>

          {/* Models */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <HugeiconsIcon icon={Settings02Icon} strokeWidth={2} className="size-4 text-muted-foreground" />
                  <CardTitle>模型列表</CardTitle>
                </div>
                <Badge variant="secondary" className="text-[10px]">
                  {formData.models.length}
                </Badge>
              </div>
              <CardDescription>配置可用的模型</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-3">
                <div className="flex gap-2">
                  <Input
                    value={newModel}
                    onChange={(e) => setNewModel(e.target.value)}
                    placeholder="输入模型名称后回车添加"
                    className="flex-1"
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
                    <HugeiconsIcon icon={Add01Icon} strokeWidth={2} className="size-3.5" />
                  </Button>
                </div>

                {formData.models.length > 0 && (
                  <div className="flex flex-wrap gap-1.5">
                    {formData.models.map((model) => (
                      <Badge
                        key={model}
                        variant={model === formData.default_model ? "default" : "secondary"}
                        className="cursor-pointer pr-1 transition-colors hover:opacity-80"
                        onClick={() => handleSetDefaultModel(model)}
                      >
                        {model}
                        {model === formData.default_model && (
                          <span className="ml-1 text-[10px] opacity-70">默认</span>
                        )}
                        <button
                          type="button"
                          className="ml-1 hover:text-destructive transition-colors cursor-pointer"
                          onClick={(e) => {
                            e.stopPropagation()
                            handleRemoveModel(model)
                          }}
                        >
                          <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} className="size-3" />
                        </button>
                      </Badge>
                    ))}
                  </div>
                )}

                <p className="text-[11px] text-muted-foreground">
                  点击标签设为默认模型，添加模型后自动测试连接
                </p>
              </div>
            </CardContent>
          </Card>
        </div>

        {/* Right - Test Results */}
        <div className="col-span-3">
          <Card className="h-full">
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <HugeiconsIcon icon={LinkIcon} strokeWidth={2} className="size-4 text-muted-foreground" />
                <CardTitle>连接测试</CardTitle>
              </div>
              <CardDescription>测试各模型的连接状态</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="border rounded-lg divide-y min-h-[400px]">
                {formData.models.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-full text-muted-foreground py-16">
                    <HugeiconsIcon icon={LinkIcon} strokeWidth={1.5} className="size-8 mb-3 opacity-40" />
                    <p className="text-sm">添加模型后将自动测试连接</p>
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
                        <div className="flex items-center justify-between px-4 py-3">
                          <div className="flex items-center gap-3 min-w-0">
                            <Badge
                              variant={model === formData.default_model ? "default" : "secondary"}
                              className="shrink-0"
                            >
                              {model}
                            </Badge>
                            {isTesting && (
                              <HugeiconsIcon icon={Loading03Icon} strokeWidth={2} className="size-4 animate-spin text-muted-foreground" />
                            )}
                            {isSuccess && (
                              <HugeiconsIcon icon={CheckmarkCircle02Icon} strokeWidth={2} className="size-4 text-emerald-500" />
                            )}
                            {isFailed && (
                              <HugeiconsIcon icon={AlertCircleIcon} strokeWidth={2} className="size-4 text-destructive" />
                            )}
                            {result && !isTesting && (
                              <span className="text-xs text-muted-foreground font-mono">
                                {result.latency_ms}ms
                              </span>
                            )}
                          </div>
                          <div className="flex items-center gap-2 shrink-0">
                            {hasDetails && (
                              <CollapsibleTrigger className="h-7 px-3 text-xs text-muted-foreground hover:text-foreground hover:bg-muted/50 rounded border-0 cursor-pointer bg-transparent transition-colors">
                                详情
                              </CollapsibleTrigger>
                            )}
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => void handleRetestModel(model)}
                              disabled={isTesting || !formData.api_key.trim()}
                            >
                              {isTesting ? "测试中..." : "重测"}
                            </Button>
                          </div>
                        </div>
                        <CollapsibleContent>
                          <div className="px-4 pb-4 pt-2 border-t space-y-3">
                            {result?.request != null && (
                              <div>
                                <p className="text-[10px] font-medium text-muted-foreground mb-1.5 uppercase tracking-wide">
                                  请求
                                </p>
                                <pre className="overflow-x-auto rounded-md bg-muted/30 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">
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
                                <p className="text-[10px] font-medium text-muted-foreground mb-1.5 uppercase tracking-wide">
                                  响应
                                </p>
                                <pre className="overflow-x-auto rounded-md bg-muted/30 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">
                                  {result!.response}
                                </pre>
                              </div>
                            )}
                            {isFailed && result?.message && (
                              <div>
                                <p className="text-[10px] font-medium text-destructive mb-1.5 uppercase tracking-wide">
                                  错误信息
                                </p>
                                <pre className="overflow-x-auto rounded-md bg-destructive/5 p-3 text-destructive font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">
                                  {result!.message}
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
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}

// FieldContent helper component
function FieldContent({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="field-content"
      className={cn("flex flex-1 flex-col gap-0.5 leading-snug", className)}
      {...props}
    />
  )
}

// Helper function
function cn(...classes: (string | boolean | undefined)[]) {
  return classes.filter(Boolean).join(" ")
}
