"use client"

import * as React from "react"
import { useRouter } from "next/navigation"
import { Save, Plus, X, Check, AlertCircle, Loader2, ArrowLeft, Cloud, Globe, Key, List, Activity } from "lucide-react"
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
import { useToast } from "@/components/ui/toast"
import { cn } from "@/lib/utils"

export interface LlmProviderRecord {
  id: number
  kind: "openai-compatible"
  display_name: string
  base_url: string
  api_key: string
  models: string[]
  model_config: Record<string, { max_context_window: number }>
  default_model: string
  is_default: boolean
  extra_headers: Record<string, string>
  secret_status: ProviderSecretStatus
  meta_data: Record<string, string>
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
    model_config: {},
    default_model: "",
    is_default: false,
    extra_headers: {},
    secret_status: "ready",
    meta_data: {},
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

  // Test connection state
  const [testResults, setTestResults] = React.useState<Record<string, ProviderTestResult>>({})
  const [testingModels, setTestingModels] = React.useState<Set<string>>(new Set())
  const [hasAutoTested, setHasAutoTested] = React.useState(false)

  // Load provider data
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
              model_config: provider.model_config ?? {},
              default_model: provider.default_model,
              is_default: provider.is_default,
              extra_headers: provider.extra_headers,
              secret_status: provider.secret_status,
              meta_data: provider.meta_data ?? {},
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

  // Auto-test all models
  React.useEffect(() => {
    if (!loading && isEditing && formData.models.length > 0 && !hasAutoTested) {
      setHasAutoTested(true)
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
        model_config: formData.model_config,
        default_model: formData.default_model,
        is_default: formData.is_default,
        extra_headers: formData.extra_headers,
        secret_status: formData.secret_status,
        meta_data: formData.meta_data,
      }
      await providers.upsert(input)
      addToast("success", isEditing ? "提供者已更新" : "提供者已创建")
      router.push("/settings/providers")
    } catch (error) {
      console.error("Failed to save provider:", error)
      addToast("error", "保存失败")
    } finally {
      setSaving(false)
    }
  }

  const handleSetDefaultModel = (model: string) => {
    setFormData({ ...formData, default_model: model })
  }

  const handleAddHeader = () => {
    const name = newHeaderName.trim()
    if (!name || formData.extra_headers[name] !== undefined) return
    setFormData({
      ...formData,
      extra_headers: { ...formData.extra_headers, [name]: newHeaderValue.trim() },
    })
    setNewHeaderName("")
    setNewHeaderValue("")
  }

  const handleRemoveHeader = (name: string) => {
    const next = { ...formData.extra_headers }
    delete next[name]
    setFormData({ ...formData, extra_headers: next })
  }

  const handleAddModel = async () => {
    const model = newModel.trim()
    if (!model || formData.models.includes(model)) return
    const newModels = [...formData.models, model]
    const newModelConfig = {
      ...formData.model_config,
      [model]: { max_context_window: 128000 },
    }
    setFormData({
      ...formData,
      models: newModels,
      model_config: newModelConfig,
      default_model: formData.default_model || model,
    })
    setNewModel("")
    if (formData.base_url.trim() && formData.api_key.trim()) {
      await testModel(model, newModels)
    }
  }

  const testModel = async (model: string, models: string[]) => {
    setTestingModels((prev) => new Set(prev).add(model))
    try {
      const input: ProviderInput = {
        ...formData,
        models: models,
        default_model: formData.default_model || model,
        meta_data: formData.meta_data,
      }
      const result = formData.id > 0
        ? await providers.testConnection(formData.id, model)
        : await providers.testInput(input, model)
      setTestResults((prev) => ({ ...prev, [model]: result }))
    } catch (error) {
      setTestResults((prev) => ({
        ...prev,
        [model]: {
          provider_id: String(formData.id),
          model,
          base_url: formData.base_url,
          checked_at: new Date().toISOString(),
          latency_ms: 0,
          status: "request_failed",
          message: String(error),
        }
      }))
    } finally {
      setTestingModels((prev) => {
        const next = new Set(prev); next.delete(model); return next
      })
    }
  }

  const handleRemoveModel = (model: string) => {
    const newModels = formData.models.filter((m) => m !== model)
    const newModelConfig = { ...formData.model_config }
    delete newModelConfig[model]
    setFormData({
      ...formData,
      models: newModels,
      model_config: newModelConfig,
      default_model: formData.default_model === model ? (newModels[0] || "") : formData.default_model,
    })
  }

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载...</div>
      </div>
    )
  }

  return (
    <div className="w-full h-full flex flex-col min-h-0 animate-in fade-in duration-500 overflow-hidden">
      {/* 顶部标题栏 */}
      <div className="flex items-center justify-between border-b pb-6 shrink-0 px-1">
        <div className="flex items-center gap-4">
          <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full hover:bg-muted" onClick={() => router.push("/settings/providers")}>
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <div className="space-y-0.5">
            <h1 className="text-lg font-bold tracking-tight">{isEditing ? "编辑提供者" : "新建提供者"}</h1>
            <p className="text-[11px] text-muted-foreground uppercase tracking-wider font-semibold opacity-70">
              Provider Configuration / {isEditing ? formData.display_name : "New Provider"}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="sm" onClick={() => router.push("/settings/providers")} className="h-9 text-sm text-muted-foreground hover:text-foreground">取消</Button>
          <Button size="sm" onClick={handleSubmit} disabled={saving || !canSave} className="h-9 px-6 text-sm font-bold shadow-lg shadow-primary/20">
            <Save className="h-4 w-4 mr-2" />
            {saving ? "正在保存..." : "保存配置"}
          </Button>
        </div>
      </div>

      {/* 核心滚动区域 */}
      <div className="flex-1 overflow-y-auto custom-scrollbar px-1 py-8">
        <div className="space-y-10 pb-20">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-8 items-stretch">
            {/* 基础连接设置 */}
            <div className="flex flex-col space-y-4 h-full">
              <div className="flex items-center gap-2 text-[11px] font-bold text-primary uppercase tracking-widest px-1">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary"><Globe className="h-3.5 w-3.5" /></div>
                Connection Settings
              </div>
              <div className="flex-1 grid gap-6 bg-muted/20 p-6 rounded-[24px] border border-muted/60 shadow-sm">
                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">显示名称</Label>
                  <Input value={formData.display_name} onChange={(e) => setFormData({ ...formData, display_name: e.target.value })} placeholder="例如: DeepSeek Official" className="h-10 bg-background border-muted/60 text-sm" />
                </div>
                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">Base URL</Label>
                  <Input value={formData.base_url} onChange={(e) => setFormData({ ...formData, base_url: e.target.value })} placeholder="https://api.deepseek.com/v1" className="h-10 bg-background border-muted/60 text-sm font-mono" />
                </div>
                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">API Key</Label>
                  <div className="relative">
                    <Key className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground/50" />
                    <Input type="password" value={formData.api_key} onChange={(e) => setFormData({ ...formData, api_key: e.target.value })} placeholder="sk-..." className="h-10 pl-9 bg-background border-muted/60 text-sm font-mono" />
                  </div>
                </div>
                <div className="flex items-center gap-3 bg-background/50 p-3 rounded-xl border border-muted/40 h-14 shadow-inner mt-2">
                  <Checkbox id="is_default" checked={formData.is_default} onCheckedChange={(v) => setFormData({ ...formData, is_default: !!v })} />
                  <Label htmlFor="is_default" className="text-sm cursor-pointer font-bold">设为全局默认提供者</Label>
                </div>
                <div className="flex items-center gap-3 bg-background/50 p-3 rounded-xl border border-muted/40 h-14 shadow-inner">
                  <Checkbox
                    id="account_token_source"
                    checked={formData.meta_data.account_token_source === "true"}
                    onCheckedChange={(v) => setFormData({ ...formData, meta_data: { ...formData.meta_data, account_token_source: !!v ? "true" : "" } })}
                  />
                  <Label htmlFor="account_token_source" className="text-sm cursor-pointer font-bold">使用账号鉴权</Label>
                </div>
              </div>
            </div>

            {/* 模型列表 */}
            <div className="flex flex-col space-y-4 h-full">
              <div className="flex items-center gap-2 text-[11px] font-bold text-primary uppercase tracking-widest px-1">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary"><List className="h-3.5 w-3.5" /></div>
                Available Models
              </div>
              <div className="flex-1 flex flex-col gap-6 bg-muted/20 p-6 rounded-[24px] border border-muted/60 shadow-sm">
                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">添加新模型</Label>
                  <div className="flex gap-2">
                    <Input value={newModel} onChange={(e) => setNewModel(e.target.value)} onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), handleAddModel())} placeholder="模型 ID (如 gpt-4)" className="h-10 bg-background border-muted/60 text-sm font-mono" />
                    <Button variant="outline" className="h-10 w-10 p-0 rounded-xl" onClick={handleAddModel} disabled={!newModel.trim()}><Plus className="h-4 w-4" /></Button>
                  </div>
                </div>
                <div className="flex-1 min-h-[120px] bg-background/50 rounded-xl border border-muted/40 p-4 overflow-y-auto shadow-inner">
                  {formData.models.length === 0 ? (
                    <div className="h-full flex items-center justify-center text-xs text-muted-foreground italic opacity-60 font-medium">尚未添加任何模型，请从上方输入并添加</div>
                  ) : (
                    <div className="space-y-2">
                      {formData.models.map((m) => (
                        <div key={m} className="flex items-center gap-2">
                          <button
                            className="flex-1 flex items-center gap-2 cursor-pointer"
                            onClick={() => handleSetDefaultModel(m)}
                          >
                            <Badge variant={m === formData.default_model ? "default" : "secondary"} className="h-7 pl-3 pr-1 rounded-lg font-mono text-[11px]">
                              {m}
                              {m === formData.default_model && <span className="ml-1.5 text-[9px] font-bold bg-primary-foreground/20 px-1 rounded uppercase tracking-tighter">Default</span>}
                            </Badge>
                          </button>
                          <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
                            <span className="whitespace-nowrap">Context:</span>
                            <input
                              type="number"
                              className="w-20 h-7 px-2 text-[11px] font-mono bg-background border border-muted/60 rounded-md text-center focus:outline-none focus:ring-1 focus:ring-primary"
                              value={formData.model_config[m]?.max_context_window ?? 128000}
                              onChange={(e) => {
                                const val = parseInt(e.target.value, 10)
                                if (!isNaN(val) && val > 0) {
                                  setFormData({
                                    ...formData,
                                    model_config: { ...formData.model_config, [m]: { max_context_window: val } },
                                  })
                                }
                              }}
                              onClick={(e) => e.stopPropagation()}
                            />
                            <span className="text-[10px]">tokens</span>
                          </div>
                          <button
                            className="p-1 rounded-md hover:bg-destructive hover:text-destructive-foreground text-muted-foreground opacity-40 hover:opacity-100 transition-all"
                            onClick={(e) => { e.stopPropagation(); handleRemoveModel(m) }}
                          >
                            <X className="h-3 w-3" />
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
                <p className="text-[10px] text-muted-foreground leading-tight font-medium opacity-80">💡 技巧：点击模型标签可切换默认模型。模型添加后将自动进行连通性测试。</p>
              </div>
            </div>
          </div>

          {/* 连接测试详情 - 全宽 */}
          <div className="space-y-4">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground px-1">
              <div className="bg-primary/10 p-1.5 rounded-lg text-primary"><Activity className="h-4 w-4" /></div>
              实时连接测试详情
            </div>
            <div className="bg-muted/10 rounded-[32px] border border-muted/60 overflow-hidden shadow-sm divide-y divide-muted/40">
              {formData.models.length === 0 ? (
                <div className="py-20 text-center text-sm text-muted-foreground">添加模型后将在此实时显示测试反馈</div>
              ) : (
                formData.models.map((model) => {
                  const result = testResults[model]; const isTesting = testingModels.has(model)
                  const isSuccess = result?.status === "success"; const isFailed = result && result.status !== "success"
                  return (
                    <Collapsible key={model} className="w-full group">
                      <div className="flex items-center justify-between px-8 py-4 hover:bg-muted/30 transition-colors">
                        <div className="flex items-center gap-4">
                          <Badge variant={model === formData.default_model ? "default" : "outline"} className="font-mono">{model}</Badge>
                          {isTesting ? <Loader2 className="h-4 w-4 animate-spin text-primary" /> :
                           isSuccess ? <div className="flex items-center gap-1.5 text-xs text-emerald-600 font-bold"><Check className="h-4 w-4" /> 成功 · {result.latency_ms}ms</div> :
                           isFailed ? <div className="flex items-center gap-1.5 text-xs text-destructive font-bold"><AlertCircle className="h-4 w-4" /> 失败</div> :
                           <span className="text-xs text-muted-foreground">待测试</span>}
                        </div>
                        <div className="flex items-center gap-2">
                          {result && (
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-8 text-xs"
                              render={<CollapsibleTrigger />}
                            >
                              查看报文
                            </Button>
                          )}
                          <Button variant="ghost" size="sm" className="h-8 text-xs hover:bg-primary/5 hover:text-primary" onClick={() => testModel(model, formData.models)} disabled={isTesting || !formData.api_key.trim()}>重新测试</Button>
                        </div>
                      </div>
                      <CollapsibleContent className="bg-muted/20 border-t border-muted/40">
                        <div className="p-8 grid grid-cols-1 lg:grid-cols-2 gap-6">
                          {result?.request && (
                            <div className="space-y-2">
                              <Label className="text-[10px] uppercase font-bold text-muted-foreground ml-1">Request Payload</Label>
                              <pre className="p-4 rounded-2xl bg-background border border-muted/60 font-mono text-[10px] overflow-auto max-h-64">{result.request}</pre>
                            </div>
                          )}
                          {result?.response && (
                            <div className="space-y-2">
                              <Label className="text-[10px] uppercase font-bold text-muted-foreground ml-1">Response Data</Label>
                              <pre className="p-4 rounded-2xl bg-background border border-muted/60 font-mono text-[10px] overflow-auto max-h-64">{result.response}</pre>
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
    </div>
  )
}
