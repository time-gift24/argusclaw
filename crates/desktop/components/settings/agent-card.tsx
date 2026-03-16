"use client"

import type { ReactNode } from "react"
import { Bot, CircleHelp, Pencil, Trash2 } from "lucide-react"
import type { LlmProviderSummary, LlmModelRecord } from "@/lib/tauri"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

export interface AgentRecord {
  id: string
  display_name: string
  description: string
  version: string
  provider_id: string
  system_prompt: string
  tool_names: string[]
  max_tokens?: number
  temperature?: number
  model_id?: string
}

interface AgentCardProps {
  agent: AgentRecord
  providers: LlmProviderSummary[]
  models: LlmModelRecord[]
  onEdit: (id: string) => void
  onDelete: (id: string) => void
}

interface DetailRowProps {
  label: ReactNode
  children: ReactNode
}

const DEFAULT_MAX_TOKENS = 4096
const DEFAULT_TEMPERATURE = 0.7

function DetailRow({ label, children }: DetailRowProps) {
  return (
    <div className="grid grid-cols-[88px_1fr] items-start gap-3">
      <span className="text-[11px] font-medium text-muted-foreground">
        {label}
      </span>
      <div className="min-w-0 text-xs">{children}</div>
    </div>
  )
}

function formatMaxTokens(maxTokens?: number) {
  const effectiveMaxTokens = maxTokens ?? DEFAULT_MAX_TOKENS
  const valueInK = effectiveMaxTokens / 1024
  const formattedValue = Number.isInteger(valueInK)
    ? valueInK.toString()
    : valueInK.toFixed(1).replace(/\.0$/, "")

  return `${formattedValue}K${maxTokens === undefined ? "（默认）" : ""}`
}

function formatTemperature(temperature?: number) {
  const effectiveTemperature = temperature ?? DEFAULT_TEMPERATURE

  return `${effectiveTemperature}${temperature === undefined ? "（默认）" : ""}`
}

export function AgentCard({ agent, providers, models, onEdit, onDelete }: AgentCardProps) {
  const provider =
    providers.find((p) => p.id === agent.provider_id)
  const providerName = provider?.display_name || agent.provider_id || "未指定"

  const model = agent.model_id
    ? models.find((m) => m.id === agent.model_id)
    : null
  const modelName = model?.name || (agent.model_id ? agent.model_id : null)

  const toolNames = agent.tool_names.filter(Boolean)
  const hasToolFilter = toolNames.length > 0

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Bot className="h-5 w-5 text-muted-foreground" />
            <CardTitle className="text-base">{agent.display_name}</CardTitle>
          </div>
          <Badge variant="outline" className="text-xs">
            v{agent.version}
          </Badge>
        </div>
        <CardDescription className="text-xs">{agent.id}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <p className="text-muted-foreground line-clamp-2">{agent.description}</p>
        <div className="rounded-md border border-border/60 bg-muted/20 p-3">
          <div className="space-y-3">
            <DetailRow label="提供者">
              <div className="min-w-0">
                <div className="truncate font-medium">{providerName}</div>
                {modelName && (
                  <div className="text-[11px] text-muted-foreground mt-0.5">
                    模型: <span className="font-mono">{modelName}</span>
                  </div>
                )}
                {agent.provider_id && providerName !== agent.provider_id && !modelName && (
                  <div className="truncate font-mono text-[11px] text-muted-foreground">
                    {agent.provider_id}
                  </div>
                )}
              </div>
            </DetailRow>

            <DetailRow label="工具">
              {hasToolFilter ? (
                <div className="flex flex-wrap gap-1">
                  {toolNames.map((tool) => (
                    <Badge key={tool} variant="secondary" className="text-xs">
                      {tool}
                    </Badge>
                  ))}
                </div>
              ) : (
                <span className="text-muted-foreground">全部可用</span>
              )}
            </DetailRow>

            <DetailRow
              label={(
                <span className="inline-flex items-center gap-1">
                  <span>最大 Token</span>
                  <Tooltip>
                    <TooltipTrigger
                      render={(
                        <button
                          type="button"
                          className="inline-flex size-3 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                          aria-label="最大 Token 说明"
                        />
                      )}
                    >
                      <CircleHelp className="size-3" />
                    </TooltipTrigger>
                    <TooltipContent side="top">模型单次 turn 允许返回的最大 token</TooltipContent>
                  </Tooltip>
                </span>
              )}
            >
              <span className="font-mono text-xs">{formatMaxTokens(agent.max_tokens)}</span>
            </DetailRow>

            <DetailRow label="温度">
              <span className="font-mono text-xs">{formatTemperature(agent.temperature)}</span>
            </DetailRow>
          </div>
        </div>
      </CardContent>
      <CardFooter className="gap-2">
        <Button size="sm" variant="outline" onClick={() => onEdit(agent.id)}>
          <Pencil className="h-3 w-3 mr-1" />
          编辑
        </Button>
        <Button size="sm" variant="destructive" onClick={() => onDelete(agent.id)}>
          <Trash2 className="h-3 w-3 mr-1" />
          删除
        </Button>
      </CardFooter>
    </Card>
  )
}
