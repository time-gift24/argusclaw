import * as React from "react"
import { MessageProvider, MessagePrimitive, type ThreadAssistantMessage } from "@assistant-ui/react"
import { useNavigate } from "react-router-dom"
import { Save, ArrowLeft, Bot, Cpu, Wrench, Settings, Eye, Server, ChevronLeft, ChevronRight } from "lucide-react"
import {
  agents,
  providers,
  tools,
  mcp,
  type AgentMcpBinding,
  type AgentRecord,
  type LlmProviderSummary,
  type McpDiscoveredToolRecord,
  type McpServerRecord,
  type ToolInfo,
} from "@/lib/tauri"

import { MarkdownText } from "@/components/assistant-ui/markdown-text"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useToast } from "@/components/ui/toast"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { AgentMcpBindingCard } from "@/components/settings/agent-mcp-binding-card"
import { cn } from "@/lib/utils"

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
    model_id: null,
    system_prompt: "",
    tool_names: [],
    subagent_names: [],
    max_tokens: undefined,
    temperature: undefined,
    thinking_config: { type: "enabled", clear_thinking: false },
  }
}

const TOOL_PAGE_SIZE = 8

interface ToolParameterDetail {
  name: string
  required: boolean
  typeLabel: string
  description: string
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

function readToolSchemaDescription(parameters: ToolInfo["parameters"]): string | null {
  const schema = asRecord(parameters)
  const description = schema?.description
  if (typeof description !== "string") return null
  const trimmed = description.trim()
  return trimmed.length > 0 ? trimmed : null
}

function toTypeLabel(schema: Record<string, unknown>): string {
  const typeField = schema.type
  if (typeof typeField === "string" && typeField.trim().length > 0) {
    return typeField
  }
  if (Array.isArray(typeField)) {
    const typeList = typeField.filter(
      (entry): entry is string => typeof entry === "string" && entry.trim().length > 0,
    )
    if (typeList.length > 0) return typeList.join(" | ")
  }
  if (Array.isArray(schema.enum) && schema.enum.length > 0) return "enum"
  return "any"
}

function extractToolParameterDetails(parameters: ToolInfo["parameters"]): ToolParameterDetail[] {
  const schema = asRecord(parameters)
  const properties = asRecord(schema?.properties)
  if (!properties) return []

  const requiredField = Array.isArray(schema?.required) ? schema?.required : []
  const required = new Set(
    requiredField.filter(
      (value): value is string => typeof value === "string" && value.trim().length > 0,
    ),
  )

  return Object.entries(properties).map(([name, propertySchema]) => {
    const propertyRecord = asRecord(propertySchema) ?? {}
    const descriptionField = propertyRecord.description
    return {
      name,
      required: required.has(name),
      typeLabel: toTypeLabel(propertyRecord),
      description:
        typeof descriptionField === "string" && descriptionField.trim().length > 0
          ? descriptionField
          : "无描述",
    }
  })
}

async function loadMcpServers() {
  return await mcp.listServers()
}

async function loadAgentMcpBindings(agentId: number) {
  return await mcp.listAgentBindings(agentId)
}

export function AgentEditor({ agentId }: AgentEditorProps) {
  const navigate = useNavigate()
  const { addToast } = useToast()
  const isEditing = agentId !== undefined

  const [loading, setLoading] = React.useState(isEditing)
  const [saving, setSaving] = React.useState(false)
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [toolList, setToolList] = React.useState<ToolInfo[]>([])
  const [mcpServerList, setMcpServerList] = React.useState<McpServerRecord[]>([])
  const [mcpBindings, setMcpBindings] = React.useState<AgentMcpBinding[]>([])
  const [mcpToolsByServerId, setMcpToolsByServerId] = React.useState<Record<number, McpDiscoveredToolRecord[]>>({})
  const mcpToolsByServerIdRef = React.useRef<Record<number, McpDiscoveredToolRecord[]>>({})
  const [availableSubagents, setAvailableSubagents] = React.useState<AgentRecord[]>([])
  const [mcpDialogOpen, setMcpDialogOpen] = React.useState(false)
  const [loadingMcpToolsByServerId, setLoadingMcpToolsByServerId] = React.useState<Record<number, boolean>>({})

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
  const uniqueToolList = React.useMemo(
    () => [...new Map(toolList.map((tool) => [tool.name, tool])).values()],
    [toolList],
  )
  const missingSubagentNames = React.useMemo(
    () =>
      formData.subagent_names.filter(
        (name) => !availableSubagents.some((agent) => agent.display_name === name),
      ),
    [availableSubagents, formData.subagent_names],
  )
  const [toolPage, setToolPage] = React.useState(1)
  const toolTotalPages = React.useMemo(
    () => Math.max(1, Math.ceil(uniqueToolList.length / TOOL_PAGE_SIZE)),
    [uniqueToolList.length],
  )
  const pagedToolList = React.useMemo(() => {
    const startIndex = (toolPage - 1) * TOOL_PAGE_SIZE
    return uniqueToolList.slice(startIndex, startIndex + TOOL_PAGE_SIZE)
  }, [toolPage, uniqueToolList])
  const mcpEnabledCount = React.useMemo(() => mcpBindings.length, [mcpBindings])
  const selectedMcpToolCount = React.useMemo(
    () =>
      mcpBindings.reduce((count, binding) => {
        if (binding.allowed_tools === null) {
          return count + (mcpToolsByServerId[binding.server_id]?.length ?? 0)
        }
        return count + binding.allowed_tools.length
      }, 0),
    [mcpBindings, mcpToolsByServerId],
  )

  React.useEffect(() => {
    setToolPage((prev) => Math.min(prev, toolTotalPages))
  }, [toolTotalPages])

  const canSave = Boolean(
    formData.display_name.trim() &&
      formData.system_prompt.trim(),
  )

  React.useEffect(() => {
    mcpToolsByServerIdRef.current = mcpToolsByServerId
  }, [mcpToolsByServerId])

  const loadMcpTools = React.useCallback(async (serverId: number) => {
    if (mcpToolsByServerIdRef.current[serverId]) return
    setLoadingMcpToolsByServerId((prev) => ({ ...prev, [serverId]: true }))
    try {
      const discoveredTools = await mcp.listServerTools(serverId)
      setMcpToolsByServerId((prev) => ({ ...prev, [serverId]: discoveredTools }))
    } catch (error) {
      console.error("Failed to load MCP tools:", error)
    } finally {
      setLoadingMcpToolsByServerId((prev) => ({ ...prev, [serverId]: false }))
    }
  }, [])

  const toggleMcpServerBinding = React.useCallback(async (serverId: number) => {
    const alreadyBound = mcpBindings.some((binding) => binding.server_id === serverId)
    if (alreadyBound) {
      setMcpBindings((prev) => prev.filter((binding) => binding.server_id !== serverId))
      return
    }

    await loadMcpTools(serverId)
    setMcpBindings((prev) => [...prev, { server_id: serverId, allowed_tools: null }])
  }, [loadMcpTools, mcpBindings])

  const setServerFullAccess = React.useCallback((serverId: number, enabled: boolean) => {
    setMcpBindings((prev) =>
      prev.map((binding) => {
        if (binding.server_id !== serverId) return binding
        const discoveredTools = mcpToolsByServerIdRef.current[serverId] ?? []
        return {
          ...binding,
          allowed_tools: enabled
            ? null
            : discoveredTools.length === 0
              ? null
              : discoveredTools.map((tool) => tool.tool_name_original),
        }
      }),
    )
  }, [])

  const toggleMcpTool = React.useCallback((serverId: number, toolName: string) => {
    setMcpBindings((prev) =>
      prev.map((binding) => {
        if (binding.server_id !== serverId) return binding
        const discoveredTools = mcpToolsByServerIdRef.current[serverId] ?? []
        const baseSelection =
          binding.allowed_tools ??
          discoveredTools.map((tool) => tool.tool_name_original)
        const nextSelection = baseSelection.includes(toolName)
          ? baseSelection.filter((name) => name !== toolName)
          : [...baseSelection, toolName]
        return {
          ...binding,
          allowed_tools: nextSelection.length === 0 ? null : nextSelection,
        }
      }),
    )
  }, [])

  // Load data
  React.useEffect(() => {
    const loadData = async () => {
      try {
        const [providersData, toolsData, allAgents, mcpServersData] =
          await Promise.all([
            providers.list(),
            tools.list(),
            agents.list(),
            loadMcpServers(),
          ])
        setProviderList(providersData)
        setToolList(toolsData)
        setMcpServerList(mcpServersData)
        const candidates = allAgents
          .filter((agent) => agent.id !== agentId)
          .sort((left, right) => left.display_name.localeCompare(right.display_name))
        setAvailableSubagents(candidates)

        const preferredProviderId = getPreferredProviderId(providersData)
        if (agentId !== undefined) {
          const [agent, bindings] = await Promise.all([
            agents.get(agentId),
            loadAgentMcpBindings(agentId).catch(() => []),
          ])
          if (agent) {
            setFormData({
              ...agent,
              subagent_names: agent.subagent_names ?? [],
            })
          }
          setMcpBindings(bindings)
          await Promise.all(
            bindings.map((binding) => loadMcpTools(binding.server_id)),
          )
        } else {
          setMcpBindings([])
          setFormData(createDefaultFormData(preferredProviderId))
        }
      } catch (error) {
        console.error("Failed to load data:", error)
      } finally {
        setLoading(false)
      }
    }
    loadData()
  }, [agentId, loadMcpTools])

  const handleSave = async () => {
    if (!canSave) return
    setSaving(true)
    try {
      const cleanedFormData = {
        ...formData,
        subagent_names: formData.subagent_names.filter(
          (name) => !missingSubagentNames.includes(name),
        ),
      }
      setFormData(cleanedFormData)
      const savedId = await agents.upsert(cleanedFormData)
      if (isEditing || savedId) {
        const targetId = isEditing ? agentId! : savedId
        await mcp.setAgentBindings(targetId, mcpBindings).catch((wsError) => {
          console.error("Failed to save agent capability bindings:", wsError)
        })
      }
      addToast("success", isEditing ? "配置已保存" : "创建成功")
      navigate(`/settings/agents/edit?id=${savedId}`)
    } catch (error) {
      console.error("Failed to save agent:", error)
      addToast("error", "保存失败")
    } finally {
      setSaving(false)
    }
  }

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载配置...</div>
      </div>
    )
  }

  return (
    <div className="w-full h-full flex flex-col min-h-0 animate-in fade-in duration-500 overflow-hidden">
      {/* 顶部标题栏 - 固定 */}
      <div className="flex items-center justify-between border-b pb-6 shrink-0 px-1">
        <div className="flex items-center gap-4">
          <Button
            variant="ghost"
            size="icon"
            className="h-9 w-9 rounded-full hover:bg-muted"
            onClick={() => navigate("/settings/agents")}
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <div className="space-y-0.5">
            <h1 className="text-lg font-bold tracking-tight">
              {isEditing ? "编辑智能体" : "新建智能体"}
            </h1>
            <p className="text-[11px] text-muted-foreground uppercase tracking-wider font-semibold opacity-70">
              {isEditing ? `Agent Configuration / ${formData.display_name}` : "Agent Configuration / New Assistant"}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="sm" onClick={() => navigate("/settings/agents")} className="h-9 text-sm text-muted-foreground hover:text-foreground">
            取消
          </Button>
          <Button size="sm" onClick={handleSave} disabled={saving || !canSave} className="h-9 px-6 text-sm font-bold shadow-lg shadow-primary/20">
            <Save className="h-4 w-4 mr-2" />
            {saving ? "正在保存..." : "保存配置"}
          </Button>
        </div>
      </div>

      {/* 核心滚动区域 */}
      <div className="flex-1 overflow-y-auto custom-scrollbar px-1 py-8">
        <div className="space-y-10 pb-20">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-8 items-stretch">
            {/* 基本设置 */}
            <div className="flex flex-col h-full space-y-4">
              <div className="flex items-center gap-2 text-[11px] font-bold text-primary uppercase tracking-widest px-1">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                  <Bot className="h-3.5 w-3.5" />
                </div>
                Basic Information
              </div>
              <div className="flex-1 flex flex-col justify-between gap-6 bg-muted/20 p-6 rounded-[24px] border border-muted/60 shadow-sm">
                <div className="space-y-2">
                  <Label htmlFor="display_name" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">显示名称</Label>
                  <Input
                    id="display_name"
                    value={formData.display_name}
                    onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
                    placeholder="例如: 翻译专家"
                    className="h-10 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm"
                  />
                </div>
                <div className="grid grid-cols-3 gap-4">
                  <div className="col-span-1 space-y-2">
                    <Label htmlFor="version" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">版本</Label>
                    <Input
                      id="version"
                      value={formData.version}
                      onChange={(e) => setFormData({ ...formData, version: e.target.value })}
                      placeholder="1.0.0"
                      className="h-10 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm font-mono"
                    />
                  </div>
                  <div className="col-span-2 space-y-2">
                    <Label htmlFor="description" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">简介</Label>
                    <Input
                      id="description"
                      value={formData.description}
                      onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                      placeholder="简单的功能说明"
                      className="h-10 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm"
                    />
                  </div>
                </div>
                <div className="space-y-3">
                  <div className="space-y-1">
                    <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">可调度子代理</Label>
                    <p className="text-[11px] text-muted-foreground ml-1">
                      勾选后，当前智能体可通过 scheduler 将任务分派给这些智能体。
                    </p>
                  </div>
                  <div className="rounded-2xl border border-muted/60 bg-background/80 p-3 shadow-inner">
                    {availableSubagents.length === 0 ? (
                      <p className="text-xs text-muted-foreground">
                        当前没有其他可选智能体。
                      </p>
                    ) : (
                      <div className="max-h-40 space-y-2 overflow-y-auto pr-1 custom-scrollbar">
                        {availableSubagents.map((agent) => {
                          const selected = formData.subagent_names.includes(agent.display_name)
                          return (
                            <label
                              key={agent.id}
                              className={cn(
                                "flex cursor-pointer items-start gap-3 rounded-xl border px-3 py-2 transition-all",
                                selected
                                  ? "border-primary bg-primary/5"
                                  : "border-muted/50 hover:border-primary/30 hover:bg-muted/30",
                              )}
                            >
                              <Checkbox
                                checked={selected}
                                onCheckedChange={(checked) => {
                                  setFormData((prev) => ({
                                    ...prev,
                                    subagent_names: checked
                                      ? [...prev.subagent_names, agent.display_name]
                                      : prev.subagent_names.filter((name) => name !== agent.display_name),
                                  }))
                                }}
                              />
                              <div className="min-w-0 flex-1">
                                <div className="flex items-center gap-2">
                                  <span className="truncate text-sm font-semibold">{agent.display_name}</span>
                                  <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] font-mono text-muted-foreground">
                                    v{agent.version}
                                  </span>
                                </div>
                                <p className="mt-1 line-clamp-2 text-[11px] text-muted-foreground">
                                  {agent.description || "未填写简介"}
                                </p>
                              </div>
                            </label>
                          )
                        })}
                      </div>
                    )}
                  </div>
                  {missingSubagentNames.length > 0 && (
                    <div className="rounded-2xl border border-amber-200 bg-amber-50/80 px-3 py-2 text-[11px] text-amber-900">
                      以下子代理名称当前未在本地模板中找到，将在下次保存后移除：{missingSubagentNames.join("、")}
                    </div>
                  )}
                </div>
              </div>
            </div>

            {/* 模型与策略 */}
            <div className="flex flex-col h-full space-y-4">
              <div className="flex items-center gap-2 text-[11px] font-bold text-primary uppercase tracking-widest px-1">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                  <Cpu className="h-3.5 w-3.5" />
                </div>
                Model Parameters
              </div>
              <div className="flex-1 flex flex-col justify-between gap-6 bg-muted/20 p-6 rounded-[24px] border border-muted/60 shadow-sm">
                <div className="space-y-2">
                  <Label htmlFor="provider_id" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">模型提供者</Label>
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
                    className="flex h-10 w-full rounded-md border border-muted/60 bg-background px-3 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-primary/20 appearance-none"
                  >
                    <option value="">自动选择默认模型</option>
                    {providerList.map((p) => (
                      <option
                        key={p.id}
                        value={p.id}
                        disabled={p.secret_status === "requires_reentry" && formData.provider_id !== p.id}
                      >
                        {p.display_name} {p.is_default ? "(默认)" : ""}
                      </option>
                    ))}
                  </select>
                </div>
                {selectedProvider && selectedProvider.models && selectedProvider.models.length > 0 && (
                  <div className="space-y-2">
                    <Label htmlFor="model_id" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">
                      指定模型
                    </Label>
                    <select
                      id="model_id"
                      value={formData.model_id ?? ""}
                      onChange={(e) =>
                        setFormData((prev) => ({
                          ...prev,
                          model_id: e.target.value === selectedProvider.default_model ? null : e.target.value || null,
                        }))
                      }
                      className="flex h-10 w-full rounded-md border border-muted/60 bg-background px-3 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-primary/20 appearance-none"
                    >
                      <option value="">使用默认模型 ({selectedProvider.default_model})</option>
                      {selectedProvider.models.map((model) => (
                        <option key={model} value={model}>
                          {model} {model === selectedProvider.default_model ? "(默认)" : ""}
                        </option>
                      ))}
                    </select>
                  </div>
                )}
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <Label htmlFor="max_tokens" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">最大 Token</Label>
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
                      className="h-10 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm font-mono"
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="temperature" className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">温度 (Temp)</Label>
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
                      className="h-10 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm font-mono"
                    />
                  </div>
                </div>

                <div className="pt-2">
                  <div className="flex items-center gap-3 bg-background/50 p-3 rounded-xl border border-muted/40 h-14 shadow-inner">
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
                    <div className="flex flex-col gap-0.5 min-w-0">
                      <Label htmlFor="thinking_enabled" className="text-sm cursor-pointer font-bold truncate">思维链 (CoT)</Label>
                      <p className="text-[10px] text-muted-foreground leading-tight truncate font-medium">显示模型思考过程。</p>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* 第二排：可用工具箱（单独一行） */}
          <div className="space-y-2.5">
            <div className="flex items-center justify-between text-sm font-bold text-foreground px-0.5">
              <div className="flex items-center gap-2">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                  <Wrench className="h-4 w-4" />
                </div>
                可用工具箱
              </div>
              <div className="flex items-center gap-1.5">
                <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-widest bg-muted/40 px-2 py-0.5 rounded-full">
                  已选 {formData.tool_names.length} / {toolList.length}
                </span>
                <div className="flex items-center gap-1">
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    className="h-6 w-6"
                    disabled={toolPage <= 1}
                    onClick={() => setToolPage((prev) => Math.max(1, prev - 1))}
                  >
                    <ChevronLeft className="h-3 w-3" />
                  </Button>
                  <span className="min-w-11 text-center text-[10px] font-mono text-muted-foreground">
                    {toolPage}/{toolTotalPages}
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    className="h-6 w-6"
                    disabled={toolPage >= toolTotalPages}
                    onClick={() => setToolPage((prev) => Math.min(toolTotalPages, prev + 1))}
                  >
                    <ChevronRight className="h-3 w-3" />
                  </Button>
                </div>
              </div>
            </div>
            <div className="bg-muted/20 p-3 rounded-3xl border border-muted/60 shadow-sm">
              {uniqueToolList.length === 0 ? (
                <div className="text-center py-10 flex items-center justify-center">
                  <p className="text-xs text-muted-foreground">当前环境下没有可用的插件工具</p>
                </div>
              ) : (
                <div className="grid content-start grid-cols-2 lg:grid-cols-4 gap-2">
                  {pagedToolList.map((tool, index) => {
                    const parameterDetails = extractToolParameterDetails(tool.parameters)
                    const detailedDescription = tool.description || readToolSchemaDescription(tool.parameters) || "无描述"
                    const columnIndex = index % 4
                    const isTopRow = index < 4
                    const hoverPositionClass =
                      isTopRow
                        ? "bottom-full mb-2 left-1/2 -translate-x-1/2"
                        : columnIndex < 2
                          ? "left-full ml-2 top-1/2 -translate-y-1/2"
                          : "right-full mr-2 top-1/2 -translate-y-1/2"
                    const arrowPositionClass =
                      isTopRow
                        ? "left-1/2 -translate-x-1/2 top-full -mt-1"
                        : columnIndex < 2
                          ? "right-full -mr-1 top-1/2 -translate-y-1/2"
                          : "left-full -ml-1 top-1/2 -translate-y-1/2"

                    return (
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
                        "group relative flex min-h-[64px] items-start gap-2 rounded-xl border p-2 cursor-pointer transition-all",
                        formData.tool_names.includes(tool.name)
                          ? "border-primary bg-primary/5 shadow-inner"
                          : "border-muted/60 bg-background hover:border-primary/30"
                      )}
                    >
                      <Checkbox
                        id={`tool-${tool.name}`}
                        checked={formData.tool_names.includes(tool.name)}
                        className="mt-0.5 shrink-0"
                        onClick={(e) => e.stopPropagation()}
                      />
                      <div className="min-w-0">
                        <Label
                          htmlFor={`tool-${tool.name}`}
                          className="text-[11px] font-bold cursor-pointer block truncate"
                        >
                          {tool.name}
                        </Label>
                        <p className="text-[10px] text-muted-foreground leading-snug line-clamp-1">
                          {tool.description || "无描述"}
                        </p>
                      </div>
                      <div
                        className={cn(
                          "pointer-events-none absolute z-20 w-80 rounded-2xl border border-primary/25 bg-gradient-to-br from-background via-background to-primary/10 p-3 shadow-2xl shadow-primary/15 opacity-0 backdrop-blur-sm transition-all duration-200 group-hover:opacity-100",
                          hoverPositionClass,
                        )}
                      >
                        <div className="text-[11px] font-bold text-foreground">{tool.name}</div>
                        <div className="mt-1 text-[10px] font-semibold text-foreground/90">描述</div>
                        <p className="mt-0.5 text-[10px] leading-snug text-muted-foreground line-clamp-4">
                          {detailedDescription}
                        </p>

                        <div className="mt-2 text-[10px] font-semibold text-foreground/90">参数</div>
                        {parameterDetails.length === 0 ? (
                          <p className="mt-0.5 text-[10px] text-muted-foreground">无参数</p>
                        ) : (
                          <div className="mt-1 max-h-28 space-y-1 overflow-y-auto custom-scrollbar pr-1">
                            {parameterDetails.map((parameter) => (
                              <div key={`${tool.name}-${parameter.name}`} className="rounded-lg border border-muted/50 bg-background/70 px-2 py-1.5">
                                <div className="flex items-center gap-1.5 text-[10px]">
                                  <span className="font-semibold text-foreground break-all">
                                    {parameter.name}{parameter.required ? "*" : ""}
                                  </span>
                                  <span className="rounded-full bg-muted px-1.5 py-0.5 font-mono text-[9px] text-muted-foreground">
                                    {parameter.typeLabel}
                                  </span>
                                </div>
                                <p className="mt-0.5 text-[10px] leading-snug text-muted-foreground line-clamp-2">
                                  {parameter.description}
                                </p>
                              </div>
                            ))}
                          </div>
                        )}
                        <div
                          className={cn(
                            "absolute h-2.5 w-2.5 rotate-45 border border-primary/25 bg-background",
                            arrowPositionClass,
                          )}
                        />
                      </div>
                    </div>
                    )
                  })}
                </div>
              )}
            </div>
          </div>

          {/* 第三排：MCP 配置 */}
          <div className="space-y-4">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground px-1">
              <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                <Server className="h-4 w-4" />
              </div>
              MCP Servers
            </div>
            <div
              onClick={() => setMcpDialogOpen(true)}
              className={cn(
                "flex flex-col items-center justify-center gap-3 p-6 rounded-3xl border cursor-pointer transition-all min-h-[200px]",
                mcpEnabledCount > 0
                  ? "border-primary bg-primary/5 shadow-inner"
                  : "border-muted/60 bg-muted/20 hover:border-primary/30",
              )}
            >
              <div className={cn(
                "p-3 rounded-2xl transition-colors",
                mcpEnabledCount > 0 ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground",
              )}>
                <Server className="h-6 w-6" />
              </div>
              <div className="text-center space-y-1">
                <p className="text-xs font-bold">
                  {mcpEnabledCount > 0 ? "已绑定 MCP" : "点击配置 MCP"}
                </p>
                <p className="text-[10px] text-muted-foreground">
                  {mcpEnabledCount > 0
                    ? `${mcpEnabledCount} 个 server · ${selectedMcpToolCount} 个 tools`
                    : "为智能体绑定可动态注入的 MCP servers"}
                </p>
              </div>
            </div>
          </div>

          <Dialog open={mcpDialogOpen} onOpenChange={setMcpDialogOpen}>
            <DialogContent className="sm:max-w-3xl max-h-[80vh] flex flex-col">
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  <Server className="h-4 w-4 text-primary" />
                  MCP 配置
                </DialogTitle>
                <DialogDescription>
                  绑定设置页中已配置的 MCP server，并按 tool 配置白名单。只有后台处于 ready 状态的 server 会在实际对话中注入。
                </DialogDescription>
              </DialogHeader>

              <div className="flex-1 overflow-y-auto space-y-5 py-2 custom-scrollbar">
                {mcpServerList.length === 0 ? (
                  <div className="rounded-2xl border border-dashed border-muted/60 bg-muted/20 p-6 text-center space-y-3">
                    <p className="text-sm font-semibold">还没有 MCP Server</p>
                    <p className="text-xs text-muted-foreground">
                      先去设置页新增并测试连接，再回来为当前智能体绑定。
                    </p>
                    <Button size="sm" variant="outline" onClick={() => navigate("/settings/mcp")}>
                      前往 MCP 设置页
                    </Button>
                  </div>
                ) : (
                  <div className="space-y-4">
                    {mcpServerList.map((server) => {
                      const serverId = server.id ?? 0
                      const binding = mcpBindings.find((item) => item.server_id === serverId) ?? null
                      const discoveredTools = mcpToolsByServerId[serverId] ?? []
                      const loadingTools = loadingMcpToolsByServerId[serverId] ?? false

                      return (
                        <AgentMcpBindingCard
                          key={serverId}
                          server={server}
                          binding={binding}
                          discoveredTools={discoveredTools}
                          loadingTools={loadingTools}
                          onToggleBinding={toggleMcpServerBinding}
                          onSetFullAccess={setServerFullAccess}
                          onToggleTool={toggleMcpTool}
                          onOpenSettings={(targetServerId) => {
                            navigate(`/settings/mcp/edit?id=${targetServerId}`)
                          }}
                        />
                      )
                    })}
                  </div>
                )}
              </div>

              <DialogFooter showCloseButton>
                <Button size="sm" onClick={() => setMcpDialogOpen(false)}>
                  完成
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* 第四排：系统提示词 - 占据整宽 */}
          <div className="space-y-4 pb-10">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground px-1">
              <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                <Settings className="h-4 w-4" />
              </div>
              核心行为指令 (System Prompt)
            </div>
            <div className="bg-muted/10 rounded-[32px] border border-muted/60 overflow-hidden shadow-sm">
              <Tabs defaultValue="editor" className="w-full">
                <div className="px-8 py-3 border-b flex items-center justify-between bg-background/50 backdrop-blur-sm">
                  <TabsList className="bg-muted/50 p-1 h-9 rounded-xl">
                    <TabsTrigger value="editor" className="text-xs px-6 rounded-lg data-[state=active]:bg-background data-[state=active]:shadow-sm">
                      编写指令
                    </TabsTrigger>
                    <TabsTrigger value="preview" className="text-xs px-6 rounded-lg data-[state=active]:bg-background data-[state=active]:shadow-sm">
                      效果预览
                    </TabsTrigger>
                  </TabsList>
                  <div className="text-[10px] font-mono text-muted-foreground px-3 py-1 rounded-full bg-muted/40 uppercase tracking-tighter">
                    {formData.system_prompt.length} Characters
                  </div>
                </div>

                <TabsContent value="editor" className="m-0 p-0">
                  <textarea
                    id="system_prompt"
                    value={formData.system_prompt}
                    onChange={(e) => setFormData({ ...formData, system_prompt: e.target.value })}
                    placeholder="在此详细定义您的智能体。包含角色定位、知识范围、语言风格等。"
                    className="w-full min-h-[500px] p-8 text-sm leading-relaxed outline-none bg-transparent resize-y font-mono custom-scrollbar"
                    required
                  />
                </TabsContent>

                <TabsContent value="preview" className="m-0 p-0 bg-background min-h-[500px]">
                  <div className="max-w-screen-2xl mx-auto p-12">
                    {formData.system_prompt ? (
                      <MessageProvider message={previewMessage} index={0} isLast>
                        <div className="prose prose-sm dark:prose-invert max-w-none">
                          <MessagePrimitive.Parts components={{ Text: MarkdownText }} />
                        </div>
                      </MessageProvider>
                    ) : (
                      <div className="flex flex-col items-center justify-center py-20 gap-4">
                        <div className="bg-muted p-4 rounded-full">
                          <Eye className="h-8 w-8 text-muted-foreground/20" />
                        </div>
                        <p className="text-sm text-muted-foreground">暂无提示词内容可供渲染预览</p>
                      </div>
                    )}
                  </div>
                </TabsContent>
              </Tabs>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
