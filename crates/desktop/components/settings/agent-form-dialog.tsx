"use client"

import * as React from "react"
import { Plus, Pencil } from "lucide-react"
import { type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"

interface AgentFormDialogProps {
  agent?: AgentRecord | null
  providers: LlmProviderSummary[]
  onSubmit: (record: AgentRecord) => Promise<void>
  trigger?: React.ReactElement
}

function createDefaultFormData(providers: LlmProviderSummary[]): AgentRecord {
  const defaultProvider = providers.find((p) => p.is_default)
  return {
    id: 0,
    display_name: "",
    description: "",
    version: "1.0.0",
    provider_id: defaultProvider?.id ?? null,
    model_id: null,
    system_prompt: "",
    tool_names: [],
    subagent_names: [],
    max_tokens: undefined,
    temperature: undefined,
  }
}

export function AgentFormDialog({ agent, providers, onSubmit, trigger }: AgentFormDialogProps) {
  const [open, setOpen] = React.useState(false)
  const [loading, setLoading] = React.useState(false)
  const isEditing = !!agent

  const [formData, setFormData] = React.useState<AgentRecord>(() => {
    if (agent) {
      return agent
    }
    return createDefaultFormData(providers)
  })

  const selectedProvider = React.useMemo(
    () => providers.find((p) => p.id === formData.provider_id) ?? null,
    [formData.provider_id, providers],
  )

  React.useEffect(() => {
    if (agent) {
      setFormData(agent)
    } else {
      setFormData(createDefaultFormData(providers))
    }
  }, [agent, providers])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    try {
      await onSubmit(formData)
      setOpen(false)
    } catch (error) {
      console.error("Failed to save agent:", error)
    } finally {
      setLoading(false)
    }
  }

  const defaultTrigger = isEditing ? (
    <Button size="sm" variant="outline">
      <Pencil className="h-3 w-3" />
    </Button>
  ) : (
    <Button size="sm">
      <Plus className="h-4 w-4 mr-1" />
      新增智能体
    </Button>
  )

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {trigger === undefined ? <DialogTrigger render={defaultTrigger} /> : trigger ? <DialogTrigger render={trigger} /> : null}
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{isEditing ? "编辑智能体" : "新增智能体"}</DialogTitle>
          <DialogDescription>
            {isEditing
              ? "更新智能体配置。"
              : "配置一个新的智能体。"}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="display_name">显示名称</Label>
            <Input
              id="display_name"
              value={formData.display_name}
              onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
              placeholder="我的智能体"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="description">描述</Label>
            <Input
              id="description"
              value={formData.description}
              onChange={(e) => setFormData({ ...formData, description: e.target.value })}
              placeholder="一个乐于助人的助手"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="provider_id">提供者</Label>
            <select
              id="provider_id"
              value={formData.provider_id ?? ""}
              onChange={(e) =>
                setFormData((prev) => ({
                  ...prev,
                  provider_id: e.target.value ? parseInt(e.target.value) : null,
                  model_id: null,
                }))
              }
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <option value="">不使用提供者</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.display_name} {p.is_default ? "（默认）" : ""}
                </option>
              ))}
            </select>
          </div>
          {selectedProvider && selectedProvider.models && selectedProvider.models.length > 0 && (
            <div className="space-y-2">
              <Label htmlFor="model_id">模型</Label>
              <select
                id="model_id"
                value={formData.model_id ?? ""}
                onChange={(e) =>
                  setFormData((prev) => ({
                    ...prev,
                    model_id: e.target.value === selectedProvider.default_model ? null : e.target.value || null,
                  }))
                }
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <option value="">默认（{selectedProvider.default_model}）</option>
                {selectedProvider.models.map((model) => (
                  <option key={model} value={model}>
                    {model} {model === selectedProvider.default_model ? "（默认）" : ""}
                  </option>
                ))}
              </select>
            </div>
          )}
          <div className="space-y-2">
            <Label htmlFor="system_prompt">系统提示词</Label>
            <Textarea
              id="system_prompt"
              value={formData.system_prompt}
              onChange={(e) => setFormData({ ...formData, system_prompt: e.target.value })}
              placeholder="你是一个乐于助人的助手……"
              rows={4}
              required
            />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="max_tokens">最大 Token 数（可选）</Label>
              <Input
                id="max_tokens"
                type="number"
                value={formData.max_tokens || ""}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    max_tokens: e.target.value ? parseInt(e.target.value) : undefined,
                  })
                }
                placeholder="4096"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="temperature">温度（可选）</Label>
              <Input
                id="temperature"
                type="number"
                step="0.1"
                min="0"
                max="2"
                value={formData.temperature ?? ""}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    temperature: e.target.value ? parseFloat(e.target.value) : undefined,
                  })
                }
                placeholder="0.7"
              />
            </div>
          </div>
          <DialogFooter>
            <Button type="submit" disabled={loading}>
              {loading ? "保存中..." : isEditing ? "更新" : "创建"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
