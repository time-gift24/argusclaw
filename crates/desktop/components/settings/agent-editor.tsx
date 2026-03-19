"use client"

import * as React from "react"
import { MessageProvider, MessagePrimitive, type ThreadAssistantMessage } from "@assistant-ui/react"
import { useRouter } from "next/navigation"
import { CircleHelp, Save } from "lucide-react"
import { agents, providers, tools, type AgentRecord, type LlmProviderSummary, type ToolInfo } from "@/lib/tauri"

import { MarkdownText } from "@/components/assistant-ui/markdown-text"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

interface AgentEditorProps {
  agentId?: number
}

function getPreferredProviderId(providersData: LlmProviderSummary[]): number | null {
  return (
    providersData.find((p) => p.is_default && p.secret_status !== "requires_reentry")?.id ??
    providersData.find((p) => p.secret_status !== "requires_reentry")?.id ??
    null
  )
}

function createDefaultFormData(preferredProviderId: number | null): AgentRecord {
  return {
    id: 0,
    display_name: "",
    description: "",
    version: "1.0.0",
    provider_id: preferredProviderId,
    system_prompt: "",
    tool_names: [],
    max_tokens: undefined,
    temperature: undefined,
    thinking_config: { type: "enabled", clear_thinking: false },
  }
}

export function AgentEditor({ agentId }: AgentEditorProps) {
  const router = useRouter()
  const isEditing = agentId !== undefined

  const [loading, setLoading] = React.useState(isEditing)
  const [saving, setSaving] = React.useState(false)
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [toolList, setToolList] = React.useState<ToolInfo[]>([])

  const [formData, setFormData] = React.useState<AgentRecord>(() => createDefaultFormData(null))

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
    formData.display_name.trim() &&
      formData.system_prompt.trim(),
  )

  // Load providers and agent data (if editing)
  React.useEffect(() => {
    const loadData = async () => {
      try {
        const providersData = await providers.list()
        setProviderList(providersData)

        const toolsData = await tools.list()
        setToolList(toolsData)

        if (agentId !== undefined) {
          const agent = await agents.get(agentId)
          if (agent) {
            setFormData(agent)
          }
        } else {
          const preferredProviderId = getPreferredProviderId(providersData)
          setFormData(createDefaultFormData(preferredProviderId))
        }
      } catch (error) {
        console.error("Failed to load data:", error)
      } finally {
        setLoading(false)
      }
    }
    loadData()
  }, [agentId]) // eslint-disable-line react-hooks/exhaustive-deps

  const handleSave = async () => {
    if (!canSave) {
      return
    }

    setSaving(true)
    try {
      const savedId = await agents.upsert(formData)
      router.push(`/settings/agents/${savedId}`)
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
    <div className="w-full space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-sm font-semibold">
          {isEditing ? "编辑智能体" : "新建智能体"}
        </h1>
        <Button size="sm" onClick={handleSave} disabled={saving || !canSave}>
          <Save className="h-4 w-4 mr-1" />
          {saving ? "保存中..." : "保存"}
        </Button>
      </div>

      {/* Basic Info */}
      <div className="space-y-4">
        <div className="grid grid-cols-2 gap-4">
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

        <div className="space-y-2">
          <Label htmlFor="provider_id">LLM 提供者（可选）</Label>
          <select
            id="provider_id"
            value={formData.provider_id ?? ""}
            onChange={(e) => setFormData({ ...formData, provider_id: e.target.value ? parseInt(e.target.value) : null })}
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

        {/* Thinking Config */}
        <div className="space-y-3">
          <div className="flex items-center gap-2">
            <Checkbox
              id="thinking_enabled"
              checked={formData.thinking_config?.type === "enabled"}
              onCheckedChange={(checked) => {
                setFormData((prev) => ({
                  ...prev,
                  thinking_config: checked
                    ? { type: "enabled", clear_thinking: prev.thinking_config?.clear_thinking ?? false }
                    : { type: "disabled", clear_thinking: false },
                }))
              }}
            />
            <Label htmlFor="thinking_enabled" className="cursor-pointer font-normal">
              启用思考模式
            </Label>
          </div>

          {formData.thinking_config?.type === "enabled" && (
            <div className="ml-6 flex items-center gap-2">
              <Checkbox
                id="clear_thinking"
                checked={formData.thinking_config?.clear_thinking ?? false}
                onCheckedChange={(checked) => {
                  setFormData((prev) => ({
                    ...prev,
                    thinking_config: prev.thinking_config
                      ? { ...prev.thinking_config, clear_thinking: checked as boolean }
                      : undefined,
                  }))
                }}
              />
              <Label htmlFor="clear_thinking" className="cursor-pointer text-sm font-normal">
                清除历史思考内容
              </Label>
              <Tooltip>
                <TooltipTrigger
                  render={(
                    <button
                      type="button"
                      className="inline-flex size-4 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      aria-label="清除历史思考内容说明"
                    />
                  )}
                >
                  <CircleHelp className="size-3" />
                </TooltipTrigger>
                <TooltipContent side="top">
                  启用后，模型不会在后续对话中看到之前的思考内容
                </TooltipContent>
              </Tooltip>
            </div>
          )}
        </div>

        <div className="space-y-2">
          <Label htmlFor="tool_names">可用工具</Label>
          <div className="space-y-2 max-h-48 overflow-y-auto border rounded-md p-3">
            {toolList.length === 0 ? (
              <p className="text-xs text-muted-foreground">暂无可用工具</p>
            ) : (
              toolList.map((tool) => (
                <div key={tool.name} className="flex items-start gap-2">
                  <Checkbox
                    id={`tool-${tool.name}`}
                    aria-describedby={`tool-desc-${tool.name}`}
                    checked={formData.tool_names.includes(tool.name)}
                    onCheckedChange={(checked) => {
                      setFormData((prev) => ({
                        ...prev,
                        tool_names: checked
                          ? [...prev.tool_names, tool.name]
                          : prev.tool_names.filter((n) => n !== tool.name),
                      }))
                    }}
                  />
                  <div className="flex-1">
                    <Label htmlFor={`tool-${tool.name}`} className="text-sm font-normal cursor-pointer">
                      {tool.name}
                    </Label>
                    <p id={`tool-desc-${tool.name}`} className="text-xs text-muted-foreground">
                      {tool.description}
                    </p>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>

      {/* System Prompt */}
      <div className="grid grid-cols-2 gap-6">
        {/* Left: Textarea */}
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
