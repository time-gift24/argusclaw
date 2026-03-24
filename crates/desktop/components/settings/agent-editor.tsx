"use client"

import * as React from "react"
import { MessageProvider, MessagePrimitive, type ThreadAssistantMessage } from "@assistant-ui/react"
import { useRouter } from "next/navigation"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  SaveIcon,
  InformationCircleIcon,
  Tick02Icon,
  Add01Icon,
  Cancel01Icon,
  Settings02Icon,
  BrainIcon,
  UserIcon,
  CodeIcon,
  ArrowDown02Icon,
} from "@hugeicons/core-free-icons"
import { agents, providers, tools, type AgentRecord, type LlmProviderSummary, type ToolInfo } from "@/lib/tauri"

import { MarkdownText } from "@/components/assistant-ui/markdown-text"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useToast } from "@/components/ui/toast"
import { Badge } from "@/components/ui/badge"

interface AgentEditorProps {
  agentId?: number
  parentId?: number
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

export function AgentEditor({ agentId, parentId }: AgentEditorProps) {
  const router = useRouter()
  const { addToast } = useToast()
  const isEditing = agentId !== undefined

  const [loading, setLoading] = React.useState(isEditing)
  const [saving, setSaving] = React.useState(false)
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [toolList, setToolList] = React.useState<ToolInfo[]>([])
  const [parentAgentList, setParentAgentList] = React.useState<AgentRecord[]>([])

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
  const excludedAgentIds = React.useMemo(
    () => getExcludedAgentIds(agentId, parentAgentList),
    [agentId, parentAgentList],
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

        const allAgents = await agents.list()
        const candidates = allAgents.filter(
          (a) => !a.parent_agent_id && a.agent_type !== "subagent" && a.id !== agentId
        )
        setParentAgentList(candidates)

        const preferredProviderId = getPreferredProviderId(providersData)
        if (agentId !== undefined) {
          const agent = await agents.get(agentId)
          if (agent) {
            setFormData(agent)
          }
        } else if (parentId !== undefined) {
          // 新建模式，parentId 由 URL 传入，预填 parent_agent_id
          setFormData({ ...createDefaultFormData(preferredProviderId), parent_agent_id: parentId })
        } else {
          setFormData(createDefaultFormData(preferredProviderId))
        }
      } catch (error) {
        console.error("Failed to load data:", error)
      } finally {
        setLoading(false)
      }
    }
    loadData()
  }, [agentId, parentId]) // eslint-disable-line react-hooks/exhaustive-deps

  const handleSave = async () => {
    if (!canSave) {
      return
    }

    setSaving(true)
    try {
      const savedId = await agents.upsert(formData)
      addToast("success", isEditing ? "智能体已更新" : "智能体已创建")
      router.push(`/settings/agents/edit?id=${savedId}`)
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
    <div className="w-full space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-base font-semibold">
            {parentId !== undefined ? "新建子智能体" : isEditing && formData.parent_agent_id ? "编辑子智能体" : isEditing ? "编辑智能体" : "新建智能体"}
          </h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {isEditing ? `ID: ${agentId}` : "创建新的智能体配置"}
          </p>
        </div>
        <Button size="sm" onClick={handleSave} disabled={saving || !canSave}>
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
        {/* Left Sidebar - Config */}
        <div className="col-span-2 space-y-4">
          {/* Basic Info */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <HugeiconsIcon icon={UserIcon} strokeWidth={2} className="size-4 text-muted-foreground" />
                <CardTitle>基本信息</CardTitle>
              </div>
              <CardDescription>设置智能体的基本属性</CardDescription>
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
                    placeholder="我的智能体"
                    required
                  />
                </Field>

                <div className="grid grid-cols-2 gap-3">
                  <Field>
                    <FieldTitle>版本</FieldTitle>
                    <Input
                      id="version"
                      value={formData.version}
                      onChange={(e) => setFormData({ ...formData, version: e.target.value })}
                      placeholder="1.0.0"
                    />
                  </Field>
                  <Field>
                    <FieldTitle>描述</FieldTitle>
                    <Input
                      id="description"
                      value={formData.description}
                      onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                      placeholder="简短描述"
                    />
                  </Field>
                </div>

                <Field>
                  <FieldTitle>父智能体</FieldTitle>
                  <Select
                    value={formData.parent_agent_id?.toString() ?? ""}
                    onValueChange={(value) =>
                      setFormData({
                        ...formData,
                        parent_agent_id: value ? parseInt(value) : undefined,
                      })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="无（独立智能体）" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="">无（独立智能体）</SelectItem>
                      {parentAgentList
                        .filter((a) => !excludedAgentIds.has(a.id))
                        .map((a) => (
                          <SelectItem key={a.id} value={a.id.toString()}>
                            {a.display_name} (v{a.version})
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                </Field>
              </FieldGroup>
            </CardContent>
          </Card>

          {/* LLM Config */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <HugeiconsIcon icon={Settings02Icon} strokeWidth={2} className="size-4 text-muted-foreground" />
                <CardTitle>模型配置</CardTitle>
              </div>
              <CardDescription>配置 LLM 提供者和参数</CardDescription>
            </CardHeader>
            <CardContent>
              <FieldGroup className="gap-4">
                <Field>
                  <FieldTitle>LLM 提供者</FieldTitle>
                  <Select
                    value={formData.provider_id?.toString() ?? ""}
                    onValueChange={(value) =>
                      setFormData({ ...formData, provider_id: value ? parseInt(value) : null })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="不指定提供者" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="">不指定提供者</SelectItem>
                      {providerList.map((p) => (
                        <SelectItem
                          key={p.id}
                          value={p.id.toString()}
                          disabled={p.secret_status === "requires_reentry" && formData.provider_id !== p.id}
                        >
                          <div className="flex items-center gap-2">
                            <span>{p.display_name}</span>
                            {p.is_default && (
                              <Badge variant="secondary" className="text-[10px] px-1 py-0">默认</Badge>
                            )}
                            {p.secret_status === "requires_reentry" && (
                              <Badge variant="outline" className="text-[10px] px-1 py-0 border-amber-300 text-amber-700">
                                需重新填写
                              </Badge>
                            )}
                          </div>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {selectedProvider?.secret_status === "requires_reentry" && (
                    <FieldDescription className="text-amber-600">
                      当前 Provider 的密钥需要重新填写
                    </FieldDescription>
                  )}
                </Field>

                <div className="grid grid-cols-2 gap-3">
                  <Field>
                    <FieldTitle>
                      <div className="flex items-center gap-1">
                        最大 Token
                        <Tooltip>
                          <TooltipTrigger
                            render={(
                              <button
                                type="button"
                                className="inline-flex size-3.5 items-center justify-center rounded-sm text-muted-foreground"
                              >
                                <HugeiconsIcon icon={InformationCircleIcon} strokeWidth={2} className="size-3" />
                              </button>
                            )}
                          />
                          <TooltipContent side="top">模型单次 turn 允许返回的最大 token</TooltipContent>
                        </Tooltip>
                      </div>
                    </FieldTitle>
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
                  </Field>
                  <Field>
                    <FieldTitle>
                      <div className="flex items-center gap-1">
                        Temperature
                        <Tooltip>
                          <TooltipTrigger
                            render={(
                              <button
                                type="button"
                                className="inline-flex size-3.5 items-center justify-center rounded-sm text-muted-foreground"
                              >
                                <HugeiconsIcon icon={InformationCircleIcon} strokeWidth={2} className="size-3" />
                              </button>
                            )}
                          />
                          <TooltipContent side="top">控制输出的随机性，0-2 之间</TooltipContent>
                        </Tooltip>
                      </div>
                    </FieldTitle>
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
                  </Field>
                </div>
              </FieldGroup>
            </CardContent>
          </Card>

          {/* Advanced */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <HugeiconsIcon icon={BrainIcon} strokeWidth={2} className="size-4 text-muted-foreground" />
                <CardTitle>高级配置</CardTitle>
              </div>
              <CardDescription>思考模式等高级选项</CardDescription>
            </CardHeader>
            <CardContent>
              <FieldGroup className="gap-3">
                <Field orientation="horizontal">
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
                  <FieldContent>
                    <FieldTitle className="cursor-pointer">启用思考模式</FieldTitle>
                  </FieldContent>
                  <Tooltip>
                    <TooltipTrigger
                      render={(
                        <button
                          type="button"
                          className="inline-flex size-4 items-center justify-center rounded-sm text-muted-foreground"
                        >
                          <HugeiconsIcon icon={InformationCircleIcon} strokeWidth={2} className="size-3.5" />
                        </button>
                      )}
                    />
                    <TooltipContent side="right">
                      启用后，模型将输出思考过程
                    </TooltipContent>
                  </Tooltip>
                </Field>

                {formData.thinking_config?.type === "enabled" && (
                  <Field orientation="horizontal" className="ml-5">
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
                    <FieldContent>
                      <FieldTitle className="cursor-pointer text-muted-foreground">清除历史思考</FieldTitle>
                    </FieldContent>
                    <Tooltip>
                      <TooltipTrigger
                        render={(
                          <button
                            type="button"
                            className="inline-flex size-4 items-center justify-center rounded-sm text-muted-foreground"
                          >
                            <HugeiconsIcon icon={InformationCircleIcon} strokeWidth={2} className="size-3.5" />
                          </button>
                        )}
                      />
                      <TooltipContent side="right">
                        启用后，模型不会在后续对话中看到之前的思考内容
                      </TooltipContent>
                    </Tooltip>
                  </Field>
                )}
              </FieldGroup>
            </CardContent>
          </Card>

          {/* Tools */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <HugeiconsIcon icon={Settings02Icon} strokeWidth={2} className="size-4 text-muted-foreground" />
                  <CardTitle>可用工具</CardTitle>
                </div>
                <Badge variant="secondary" className="text-[10px]">
                  {formData.tool_names.length} / {toolList.length}
                </Badge>
              </div>
              <CardDescription>选择智能体可以使用的工具</CardDescription>
            </CardHeader>
            <CardContent>
              {toolList.length === 0 ? (
                <div className="text-sm text-muted-foreground text-center py-6">
                  暂无可用工具
                </div>
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
                          <Label
                            htmlFor={`tool-${tool.name}`}
                            className="text-xs font-medium cursor-pointer block truncate"
                          >
                            {tool.name}
                          </Label>
                          <p className="text-[11px] text-muted-foreground line-clamp-2 mt-0.5">
                            {tool.description}
                          </p>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Right Main - System Prompt */}
        <div className="col-span-3">
          <Card className="h-full">
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <HugeiconsIcon icon={CodeIcon} strokeWidth={2} className="size-4 text-muted-foreground" />
                  <CardTitle>系统提示词</CardTitle>
                </div>
                <Badge variant="outline" className="text-[10px] font-mono">
                  {formData.system_prompt.length} 字符
                </Badge>
              </div>
              <CardDescription>定义智能体的角色、能力和行为规范</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                <Textarea
                  id="system_prompt"
                  value={formData.system_prompt}
                  onChange={(e) => setFormData({ ...formData, system_prompt: e.target.value })}
                  placeholder="你是一个有帮助的助手...&#10;&#10;在这里定义智能体的角色、能力和行为规范。"
                  className="min-h-[280px] font-mono text-sm"
                  required
                />

                <div className="border rounded-lg overflow-hidden">
                  <div className="px-3 py-2 bg-muted/50 border-b flex items-center gap-2">
                    <HugeiconsIcon icon={Tick02Icon} strokeWidth={2} className="size-3.5 text-muted-foreground" />
                    <span className="text-xs font-medium text-muted-foreground">实时预览</span>
                  </div>
                  <div className="min-h-[280px] max-h-[360px] overflow-y-auto p-4">
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
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}

// Helper function
function cn(...classes: (string | boolean | undefined)[]) {
  return classes.filter(Boolean).join(" ")
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

function getExcludedAgentIds(agentId: number | undefined, allAgents: AgentRecord[]): Set<number> {
  if (agentId === undefined) return new Set()
  const excluded = new Set<number>()
  const queue = [agentId]
  while (queue.length > 0) {
    const current = queue.shift()!
    const children = allAgents.filter((a) => a.parent_agent_id === current)
    for (const child of children) {
      if (!excluded.has(child.id)) {
        excluded.add(child.id)
        queue.push(child.id)
      }
    }
  }
  return excluded
}
