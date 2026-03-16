"use client"

import * as React from "react"
import { MessageProvider, MessagePrimitive, type ThreadAssistantMessage } from "@assistant-ui/react"
import { useRouter } from "next/navigation"
import { ArrowLeft, Save } from "lucide-react"
import { agents, providers, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"

import { MarkdownText } from "@/components/assistant-ui/markdown-text"
import { Breadcrumb } from "@/components/settings"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"

interface AgentEditorProps {
  agentId?: string
}

function getPreferredProviderId(providersData: LlmProviderSummary[]): string {
  return (
    providersData.find((p) => p.is_default && p.secret_status !== "requires_reentry")?.id ||
    providersData.find((p) => p.secret_status !== "requires_reentry")?.id ||
    ""
  )
}

export function AgentEditor({ agentId }: AgentEditorProps) {
  const router = useRouter()
  const isEditing = !!agentId

  const [loading, setLoading] = React.useState(isEditing)
  const [saving, setSaving] = React.useState(false)
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])

  const [formData, setFormData] = React.useState<AgentRecord>({
    id: "",
    display_name: "",
    description: "",
    version: "1.0.0",
    provider_id: "",
    system_prompt: "",
    tool_names: [],
    max_tokens: undefined,
    temperature: undefined,
  })

  const previewMessage = React.useMemo<ThreadAssistantMessage>(
    () => ({
      id: "agent-prompt-preview",
      role: "assistant",
      createdAt: new Date(0),
      content: [{ type: "text", text: formData.system_prompt }],
      status: { type: "complete", reason: "unknown" },
      metadata: {
        unstable_state: null,
        unstable_annotations: [],
        unstable_data: [],
        steps: [],
        custom: {},
      },
    }),
    [formData.system_prompt],
  )
  const selectedProvider = React.useMemo(
    () => providerList.find((provider) => provider.id === formData.provider_id) ?? null,
    [formData.provider_id, providerList],
  )

  const canSave = Boolean(
    formData.id.trim() &&
      formData.display_name.trim() &&
      formData.system_prompt.trim(),
  )

  // Load providers and agent data (if editing)
  React.useEffect(() => {
    const loadData = async () => {
      try {
        const providersData = await providers.list()
        setProviderList(providersData)

        if (agentId) {
          const agent = await agents.get(agentId)
          if (agent) {
            setFormData(agent)
          }
        } else {
          const preferredProviderId = getPreferredProviderId(providersData)
          if (preferredProviderId) {
            setFormData((prev) =>
              prev.provider_id ? prev : { ...prev, provider_id: preferredProviderId },
            )
          }
        }
      } catch (error) {
        console.error("Failed to load data:", error)
      } finally {
        setLoading(false)
      }
    }
    loadData()
  }, [agentId])

  const handleSave = async () => {
    if (!canSave) {
      return
    }

    setSaving(true)
    try {
      await agents.upsert(formData)
      router.push("/settings/agents")
    } catch (error) {
      console.error("Failed to save agent:", error)
    } finally {
      setSaving(false)
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
    <div className="mx-auto max-w-7xl px-6 py-6 space-y-4">
      <Breadcrumb
        items={[
          { label: "设置", href: "/settings" },
          { label: "智能体", href: "/settings/agents" },
          ...(isEditing ? [{ label: formData.display_name || "新建" }] : []),
        ]}
      />

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" onClick={() => router.back()}>
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h1 className="text-sm font-semibold">
            {isEditing ? "编辑智能体" : "新建智能体"}
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
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="id">ID</Label>
              <Input
                id="id"
                value={formData.id}
                onChange={(e) => setFormData({ ...formData, id: e.target.value })}
                placeholder="my-agent"
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
                placeholder="我的智能体"
                required
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="description">描述</Label>
            <Input
              id="description"
              value={formData.description}
              onChange={(e) => setFormData({ ...formData, description: e.target.value })}
              placeholder="一个有用的智能体"
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="version">版本</Label>
              <Input
                id="version"
                value={formData.version}
                onChange={(e) => setFormData({ ...formData, version: e.target.value })}
                placeholder="1.0.0"
                required
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="provider_id">LLM 提供者（可选）</Label>
              <select
                id="provider_id"
                value={formData.provider_id}
                onChange={(e) => setFormData({ ...formData, provider_id: e.target.value })}
                className="flex h-7 w-full rounded-md border border-input bg-input/20 px-2 py-0.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 dark:bg-input/30"
              >
                <option value="">不指定提供者</option>
                {providerList.map((p) => (
                  <option
                    key={p.id}
                    value={p.id}
                    disabled={p.secret_status === "requires_reentry" && formData.provider_id !== p.id}
                  >
                    {p.display_name} {p.is_default ? "(默认)" : ""} {p.secret_status === "requires_reentry" ? "(需要重新填写 API Key)" : ""}
                  </option>
                ))}
              </select>
              {selectedProvider?.secret_status === "requires_reentry" && (
                <p className="text-xs text-amber-700">
                  当前 Provider 的密钥需要重新填写，修复前无法正常用于新会话。
                </p>
              )}
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="system_prompt">系统提示词</Label>
            <textarea
              id="system_prompt"
              value={formData.system_prompt}
              onChange={(e) => setFormData({ ...formData, system_prompt: e.target.value })}
              placeholder="你是一个有帮助的助手..."
              className="flex min-h-[400px] w-full rounded-md border border-input bg-input/20 px-3 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 dark:bg-input/30 resize-none"
              required
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="max_tokens">最大 Token (可选)</Label>
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
              <Label htmlFor="temperature">Temperature (可选)</Label>
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
        </div>

        {/* Right: Preview */}
        <div className="border rounded-lg overflow-hidden flex flex-col">
          <div className="bg-muted/50 px-4 py-2 border-b text-xs font-medium text-muted-foreground">
            预览
          </div>
          <div className="flex-1 overflow-y-auto p-4">
            {formData.system_prompt ? (
              <MessageProvider message={previewMessage} index={0} isLast>
                <div className="wrap-break-word px-2 text-foreground leading-relaxed [&_.aui-md-h3]:text-sm">
                  <MessagePrimitive.Parts components={{ Text: MarkdownText }} />
                </div>
              </MessageProvider>
            ) : (
              <div className="text-muted-foreground text-sm">
                在左侧输入系统提示词，这里将实时显示渲染效果
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
