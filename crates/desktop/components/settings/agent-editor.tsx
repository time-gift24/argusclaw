"use client"

import * as React from "react"
import { MessageProvider, MessagePrimitive, type ThreadAssistantMessage } from "@assistant-ui/react"
import { useRouter } from "next/navigation"
import { CircleHelp, Save } from "lucide-react"
import { agents, providers, tools, type AgentRecord, type LlmProviderSummary, type ToolInfo } from "@/lib/tauri"

import { MarkdownText } from "@/components/assistant-ui/markdown-text"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useToast } from "@/components/ui/toast"

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
  const { addToast } = useToast()
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
      addToast("success", isEditing ? "智能体已更新" : "智能体已创建")
      router.push(`/settings/agents/${savedId}`)
    } catch (error) {
      console.error("Failed to save agent:", error)
      addToast("error", "保存失败，请重试")
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
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-base font-semibold">
          {isEditing ? "编辑智能体" : "新建智能体"}
        </h1>
        <Button size="sm" onClick={handleSave} disabled={saving || !canSave}>
          <Save className="h-4 w-4 mr-1" />
          {saving ? "保存中..." : "保存"}
        </Button>
      </div>

      {/* Main Content */}
      <div className="grid grid-cols-5 gap-6">
        {/* Left Sidebar - Config */}
        <div className="col-span-2 space-y-4">
          {/* Basic Info */}
          <div className="rounded-lg border bg-card text-card-foreground shadow-sm">
            <div className="px-4 py-3 border-b">
              <h2 className="text-sm font-medium">基本信息</h2>
            </div>
            <div className="p-4 space-y-3">
              <div className="space-y-1.5">
                <Label htmlFor="display_name" className="text-xs">名称</Label>
                <Input
                  id="display_name"
                  value={formData.display_name}
                  onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
                  placeholder="我的智能体"
                  required
                />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1.5">
                  <Label htmlFor="version" className="text-xs">版本</Label>
                  <Input
                    id="version"
                    value={formData.version}
                    onChange={(e) => setFormData({ ...formData, version: e.target.value })}
                    placeholder="1.0.0"
                    required
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="description" className="text-xs">描述</Label>
                  <Input
                    id="description"
                    value={formData.description}
                    onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                    placeholder="简短描述"
                  />
                </div>
              </div>
            </div>
          </div>

          {/* LLM Config */}
          <div className="rounded-lg border bg-card text-card-foreground shadow-sm">
            <div className="px-4 py-3 border-b">
              <h2 className="text-sm font-medium">模型配置</h2>
            </div>
            <div className="p-4 space-y-3">
              <div className="space-y-1.5">
                <Label htmlFor="provider_id" className="text-xs">LLM 提供者</Label>
                <select
                  id="provider_id"
                  value={formData.provider_id ?? ""}
                  onChange={(e) => setFormData({ ...formData, provider_id: e.target.value ? parseInt(e.target.value) : null })}
                  className="flex h-9 w-full rounded-md border border-input bg-input/20 px-3 py-1.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30"
                >
                  <option value="">不指定提供者</option>
                  {providerList.map((p) => (
                    <option
                      key={p.id}
                      value={p.id}
                      disabled={p.secret_status === "requires_reentry" && formData.provider_id !== p.id}
                    >
                      {p.display_name} {p.is_default ? "(默认)" : ""} {p.secret_status === "requires_reentry" ? "(需要重新填写)" : ""}
                    </option>
                  ))}
                </select>
                {selectedProvider?.secret_status === "requires_reentry" && (
                  <p className="text-xs text-amber-600 mt-1">当前 Provider 的密钥需要重新填写</p>
                )}
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1.5">
                  <Label htmlFor="max_tokens" className="text-xs">最大 Token</Label>
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
                <div className="space-y-1.5">
                  <Label htmlFor="temperature" className="text-xs">Temperature</Label>
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
          </div>

          {/* Advanced */}
          <div className="rounded-lg border bg-card text-card-foreground shadow-sm">
            <div className="px-4 py-3 border-b">
              <h2 className="text-sm font-medium">高级配置</h2>
            </div>
            <div className="p-4 space-y-3">
              <div className="flex items-center justify-between">
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
                  <Label htmlFor="thinking_enabled" className="text-sm cursor-pointer">
                    启用思考模式
                  </Label>
                </div>
                <Tooltip>
                  <TooltipTrigger
                    render={(
                      <button
                        type="button"
                        className="inline-flex size-5 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-foreground"
                      >
                        <CircleHelp className="size-3.5" />
                      </button>
                    )}
                  />
                  <TooltipContent side="right">
                    启用后，模型将输出思考过程
                  </TooltipContent>
                </Tooltip>
              </div>
              {formData.thinking_config?.type === "enabled" && (
                <div className="flex items-center justify-between ml-6">
                  <div className="flex items-center gap-2">
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
                    <Label htmlFor="clear_thinking" className="text-sm cursor-pointer text-muted-foreground">
                      清除历史思考
                    </Label>
                  </div>
                  <Tooltip>
                    <TooltipTrigger
                      render={(
                        <button
                          type="button"
                          className="inline-flex size-5 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-foreground"
                        >
                          <CircleHelp className="size-3" />
                        </button>
                      )}
                    />
                    <TooltipContent side="right">
                      启用后，模型不会在后续对话中看到之前的思考内容
                    </TooltipContent>
                  </Tooltip>
                </div>
              )}
            </div>
          </div>

          {/* Tools */}
          <div className="rounded-lg border bg-card text-card-foreground shadow-sm">
            <div className="px-4 py-3 border-b flex items-center justify-between">
              <h2 className="text-sm font-medium">可用工具</h2>
              <span className="text-xs text-muted-foreground">
                {formData.tool_names.length} / {toolList.length}
                {toolList.some((t) => isMcpTool(t.name)) && (
                  <span className="ml-1 text-purple-600">
                    ({toolList.filter((t) => isMcpTool(t.name)).length} MCP)
                  </span>
                )}
              </span>
            </div>
            <div className="p-3">
              {toolList.length === 0 ? (
                <p className="text-sm text-muted-foreground text-center py-4">暂无可用工具</p>
              ) : (
                <div className="grid grid-cols-2 gap-2 max-h-[240px] overflow-y-auto pr-1">
                  {[...new Map(toolList.map((tool) => [tool.name, tool])).values()].map((tool) => (
                    <div
                      key={tool.name}
                      onClick={() => {
                        const isSelected = formData.tool_names.includes(tool.name)
                        setFormData((prev) => ({
                          ...prev,
                          tool_names: isSelected
                            ? prev.tool_names.filter((n) => n !== tool.name)
                            : [...prev.tool_names, tool.name],
                        }))
                      }}
                      className={cn(
                        "rounded-md border p-2.5 cursor-pointer transition-all text-left",
                        "hover:border-primary/50 hover:bg-primary/5",
                        formData.tool_names.includes(tool.name)
                          ? "border-primary/30 bg-primary/5"
                          : "border-border"
                      )}
                    >
                      <div className="flex items-start gap-2">
                        <Checkbox
                          id={`tool-${tool.name}`}
                          checked={formData.tool_names.includes(tool.name)}
                          onCheckedChange={(checked) => {
                            setFormData((prev) => ({
                              ...prev,
                              tool_names: checked
                                ? [...prev.tool_names, tool.name]
                                : prev.tool_names.filter((n) => n !== tool.name),
                            }))
                          }}
                          onClick={(e) => e.stopPropagation()}
                        />
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-1.5 flex-wrap">
                            <Label
                              htmlFor={`tool-${tool.name}`}
                              className="text-sm font-medium cursor-pointer block truncate"
                            >
                              {tool.name}
                            </Label>
                            {isMcpTool(tool.name) && (
                              <Badge variant="outline" className="text-[10px] px-1 py-0 h-4 border-purple-300 text-purple-600">
                                MCP
                              </Badge>
                            )}
                          </div>
                          <p className="text-xs text-muted-foreground line-clamp-2 mt-0.5">
                            {tool.description}
                          </p>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Right Main - System Prompt */}
        <div className="col-span-3">
          <div className="rounded-lg border bg-card text-card-foreground shadow-sm h-full">
            <div className="px-4 py-3 border-b flex items-center justify-between">
              <h2 className="text-sm font-medium">系统提示词</h2>
              <span className="text-xs text-muted-foreground">
                {formData.system_prompt.length} 字符
              </span>
            </div>
            <div className="p-4 space-y-4">
              <textarea
                id="system_prompt"
                value={formData.system_prompt}
                onChange={(e) => setFormData({ ...formData, system_prompt: e.target.value })}
                placeholder="你是一个有帮助的助手...&#10;&#10;在这里定义智能体的角色、能力和行为规范。"
                className="flex min-h-[300px] w-full rounded-md border border-input bg-input/20 px-3 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 resize-none font-mono"
                required
              />
              <div className="border rounded-lg overflow-hidden">
                <div className="px-3 py-2 bg-muted/50 border-b text-xs font-medium text-muted-foreground">
                  实时预览
                </div>
                <div className="min-h-[300px] max-h-[400px] overflow-y-auto p-4">
                  {formData.system_prompt ? (
                    <MessageProvider message={previewMessage} index={0} isLast>
                      <div className="wrap-break-word text-foreground leading-relaxed [&_.aui-md-h3]:text-sm">
                        <MessagePrimitive.Parts components={{ Text: MarkdownText }} />
                      </div>
                    </MessageProvider>
                  ) : (
                    <div className="text-muted-foreground text-sm text-center py-12">
                      输入系统提示词后，这里将显示渲染效果
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

// Helper function to detect if a tool is an MCP tool
function isMcpTool(toolName: string): boolean {
  return toolName.startsWith("mcp_")
}

// Helper function to parse MCP tool name into server and tool names
function parseMcpToolName(toolName: string): { serverName: string; toolName: string } | null {
  if (!isMcpTool(toolName)) return null
  // Format: mcp_{server_name}_{tool_name}
  // The server name is between mcp_ and the last underscore
  const withoutPrefix = toolName.slice(4) // Remove "mcp_"
  const lastUnderscoreIndex = withoutPrefix.lastIndexOf("_")
  if (lastUnderscoreIndex === -1) return null
  return {
    serverName: withoutPrefix.slice(0, lastUnderscoreIndex),
    toolName: withoutPrefix.slice(lastUnderscoreIndex + 1),
  }
}

// Helper function
function cn(...classes: (string | boolean | undefined)[]) {
  return classes.filter(Boolean).join(" ")
}
