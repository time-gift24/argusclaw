import { Settings2 } from "lucide-react"

import {
  type AgentMcpBinding,
  type McpDiscoveredToolRecord,
  type McpServerRecord,
} from "@/lib/tauri"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { cn } from "@/lib/utils"

interface AgentMcpBindingCardProps {
  server: McpServerRecord
  binding: AgentMcpBinding | null
  discoveredTools: McpDiscoveredToolRecord[]
  loadingTools: boolean
  onToggleBinding: (serverId: number) => void | Promise<void>
  onSetFullAccess: (serverId: number, enabled: boolean) => void
  onToggleTool: (serverId: number, toolName: string) => void
  onOpenSettings: (serverId: number) => void
}

function getStatusTone(status: McpServerRecord["status"]) {
  switch (status) {
    case "ready":
      return "bg-emerald-500/10 text-emerald-700"
    case "disabled":
      return "bg-slate-500/10 text-slate-600"
    default:
      return "bg-amber-500/10 text-amber-700"
  }
}

export function AgentMcpBindingCard({
  server,
  binding,
  discoveredTools,
  loadingTools,
  onToggleBinding,
  onSetFullAccess,
  onToggleTool,
  onOpenSettings,
}: AgentMcpBindingCardProps) {
  const serverId = server.id ?? 0
  const isBound = binding !== null
  const allToolsEnabled = binding?.allowed_tools === null
  const canConfigureTools = discoveredTools.length > 0

  return (
    <div
      className={cn(
        "rounded-2xl border p-4 space-y-4 transition-all",
        isBound ? "border-primary bg-primary/5" : "border-muted/60 bg-background",
      )}
    >
      <div className="flex items-start gap-3">
        <Checkbox
          checked={isBound}
          className="mt-0.5 shrink-0"
          onCheckedChange={() => {
            if (serverId > 0) {
              void onToggleBinding(serverId)
            }
          }}
        />
        <div
          onClick={() => {
            if (serverId > 0) {
              void onToggleBinding(serverId)
            }
          }}
          className="flex-1 min-w-0 space-y-1 cursor-pointer"
        >
          <div className="flex items-center gap-2 flex-wrap">
            <p className="text-sm font-bold truncate">{server.display_name}</p>
            <span className="text-[10px] font-mono rounded-full bg-muted px-2 py-0.5 uppercase">
              {server.transport.kind}
            </span>
            <span className={cn("text-[10px] rounded-full px-2 py-0.5 uppercase", getStatusTone(server.status))}>
              {server.status}
            </span>
          </div>
          <p className="text-[11px] text-muted-foreground">
            {server.discovered_tool_count} 个已发现工具
            {server.last_error ? ` · ${server.last_error}` : ""}
          </p>
        </div>
        <Button
          size="sm"
          variant="ghost"
          className="shrink-0"
          onClick={(event) => {
            event.stopPropagation()
            if (serverId > 0) {
              onOpenSettings(serverId)
            }
          }}
        >
          <Settings2 className="mr-1.5 h-3.5 w-3.5" />
          前往设置
        </Button>
      </div>

      {isBound && (
        <div className="space-y-3 border-t pt-4">
          <div
            className={cn(
              "flex items-center gap-3 rounded-xl border border-muted/50 bg-background/60 px-3 py-2",
              canConfigureTools ? "cursor-pointer" : "opacity-70",
            )}
            onClick={() => {
              if (serverId > 0 && canConfigureTools) {
                onSetFullAccess(serverId, !allToolsEnabled)
              }
            }}
          >
            <Checkbox
              checked={allToolsEnabled}
              disabled={!canConfigureTools}
              onClick={(event) => event.stopPropagation()}
              onCheckedChange={(checked) => {
                if (serverId > 0 && canConfigureTools) {
                  onSetFullAccess(serverId, checked === true)
                }
              }}
            />
            <div>
              <p className="text-xs font-bold">启用该 server 的全部 tools</p>
              <p className="text-[10px] text-muted-foreground">
                {canConfigureTools
                  ? "关闭后可按工具做细粒度白名单控制。"
                  : "当前还没有 discovery 快照，暂时无法做按 tool 精选。"}
              </p>
            </div>
          </div>

          {loadingTools ? (
            <div className="text-xs text-muted-foreground">正在加载工具列表...</div>
          ) : !canConfigureTools ? (
            <div className="rounded-xl border border-dashed border-muted/60 px-4 py-3 text-xs text-muted-foreground">
              该 server 还没有可用的 discovery 快照。先去 MCP 设置页测试连接以拉取工具列表。
            </div>
          ) : !allToolsEnabled ? (
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
              {discoveredTools.map((tool) => {
                const enabled = binding.allowed_tools?.includes(tool.tool_name_original) ?? false
                return (
                  <div
                    key={`${serverId}-${tool.tool_name_original}`}
                    onClick={() => onToggleTool(serverId, tool.tool_name_original)}
                    className={cn(
                      "rounded-xl border p-3 cursor-pointer transition-all",
                      enabled
                        ? "border-primary bg-primary/5"
                        : "border-muted/60 bg-background hover:border-primary/30",
                    )}
                  >
                    <div className="flex items-start gap-2">
                      <Checkbox
                        checked={enabled}
                        className="mt-0.5 shrink-0"
                        onClick={(event) => event.stopPropagation()}
                        onCheckedChange={() => onToggleTool(serverId, tool.tool_name_original)}
                      />
                      <div className="min-w-0 space-y-1">
                        <p className="text-xs font-bold truncate">{tool.tool_name_original}</p>
                        <p className="text-[10px] text-muted-foreground line-clamp-2">
                          {tool.description || "No description"}
                        </p>
                      </div>
                    </div>
                  </div>
                )
              })}
            </div>
          ) : null}
        </div>
      )}
    </div>
  )
}
