"use client"

import { Bot, Pencil, Trash2 } from "lucide-react"
import type { AgentRecord, LlmProviderSummary } from "@/lib/tauri"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

interface AgentCardProps {
  agent: AgentRecord
  providers: LlmProviderSummary[]
  onEdit: (id: number) => void
  onDelete: (id: number) => void
}

const DEFAULT_TEMPERATURE = 0.7

function formatTemperature(temperature?: number) {
  const effectiveTemperature = temperature ?? DEFAULT_TEMPERATURE
  return effectiveTemperature.toString()
}

export function AgentCard({ agent, providers, onEdit, onDelete }: AgentCardProps) {
  const providerName =
    providers.find((provider) => provider.id === agent.provider_id)?.display_name ||
    agent.provider_id ||
    "未指定"
  const dispatchCapable = agent.subagent_names.length > 0

  return (
    <Card className="group overflow-hidden border-muted/60 transition-all hover:border-primary/30 hover:shadow-md bg-background">
      <div className="flex items-center gap-3 p-3">
        {/* Left: Icon & Title Group */}
        <div className="flex flex-1 items-center gap-3 min-w-0">
          <div className="shrink-0 rounded-lg bg-primary/5 p-1.5 text-primary transition-colors group-hover:bg-primary group-hover:text-primary-foreground">
            <Bot className="h-4 w-4" />
          </div>
          <div className="flex flex-col min-w-0">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-bold truncate leading-none">{agent.display_name}</h3>
              <Badge variant="outline" className="text-[9px] h-3.5 px-1 font-mono opacity-50 shrink-0 border-muted-foreground/20 font-bold uppercase">
                v{agent.version}
              </Badge>
              <Badge className={`text-[9px] h-3.5 px-1 shrink-0 shadow-none font-bold uppercase ${
                dispatchCapable
                  ? "bg-emerald-50 text-emerald-700 border-emerald-200/50 hover:bg-emerald-50"
                  : "bg-slate-50 text-slate-600 border-slate-200/50 hover:bg-slate-50"
              }`}>
                {dispatchCapable ? "可调度" : "单体"}
              </Badge>
            </div>
            <p className="text-[11px] text-muted-foreground truncate mt-1.5 leading-none">
              {agent.description || "暂无描述内容"}
            </p>
          </div>
        </div>

        {/* Middle: Key Metrics (Hidden on small screens) */}
        <div className="hidden h-8 items-center gap-4 border-x border-muted/30 px-4 md:flex">
          <div className="flex flex-col gap-1">
            <span className="text-[9px] font-bold text-muted-foreground/50 uppercase tracking-widest leading-none">提供者</span>
            <span className="text-[11px] font-medium truncate max-w-[80px] leading-none">{providerName}</span>
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[9px] font-bold text-muted-foreground/50 uppercase tracking-widest leading-none">子代理</span>
            <span className="text-[11px] font-medium leading-none">{agent.subagent_names.length > 0 ? `${agent.subagent_names.length} 个` : "无"}</span>
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[9px] font-bold text-muted-foreground/50 uppercase tracking-widest leading-none">温度</span>
            <span className="text-[11px] font-mono font-medium leading-none">{formatTemperature(agent.temperature)}</span>
          </div>
        </div>

        {/* Right: Actions */}
        <div className="flex items-center gap-1 shrink-0">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 rounded-lg text-muted-foreground transition-colors hover:bg-primary/5 hover:text-primary"
            onClick={() => onEdit(agent.id)}
          >
            <Pencil className="h-3.5 w-3.5" />
            <span className="sr-only">编辑</span>
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 rounded-lg text-muted-foreground transition-colors hover:bg-destructive/5 hover:text-destructive"
            onClick={() => onDelete(agent.id)}
          >
            <Trash2 className="h-3.5 w-3.5" />
            <span className="sr-only">删除</span>
          </Button>
        </div>
      </div>
    </Card>
  )
}
