"use client"

import type { ReactNode } from "react"
import { Bot, Pencil, Trash2 } from "lucide-react"
import type { LlmProviderSummary } from "@/lib/tauri"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

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
}

interface AgentCardProps {
  agent: AgentRecord
  providers: LlmProviderSummary[]
  onEdit: (id: string) => void
  onDelete: (id: string) => void
}

interface DetailRowProps {
  label: string
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

export function AgentCard({ agent, providers, onEdit, onDelete }: AgentCardProps) {
  const providerName =
    providers.find((provider) => provider.id === agent.provider_id)?.display_name ||
    agent.provider_id ||
    "未指定"
  const toolNames = agent.tool_names.filter(Boolean)

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
                {agent.provider_id && providerName !== agent.provider_id ? (
                  <div className="truncate font-mono text-[11px] text-muted-foreground">
                    {agent.provider_id}
                  </div>
                ) : null}
              </div>
            </DetailRow>

            <DetailRow label="工具">
              {toolNames.length > 0 ? (
                <div className="flex flex-wrap gap-1">
                  {toolNames.map((tool) => (
                    <Badge key={tool} variant="secondary" className="text-xs">
                      {tool}
                    </Badge>
                  ))}
                </div>
              ) : (
                <span className="text-muted-foreground">未配置</span>
              )}
            </DetailRow>

            <DetailRow label="最大 Token">
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
