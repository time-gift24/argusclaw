import { Activity, Pencil, Trash2 } from "lucide-react"

import { type McpConnectionTestResult, type McpServerRecord } from "@/lib/tauri"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface McpCardProps {
  server: McpServerRecord
  latestResult?: McpConnectionTestResult | null
  testing?: boolean
  onEdit: () => void
  onTest: () => void
  onDelete: () => void
  onViewResult?: () => void
}

function getStatusTone(status: McpServerRecord["status"]) {
  switch (status) {
    case "ready":
      return "bg-emerald-500/10 text-emerald-700"
    case "disabled":
      return "bg-slate-500/10 text-slate-600"
    case "failed":
      return "bg-rose-500/10 text-rose-700"
    default:
      return "bg-amber-500/10 text-amber-700"
  }
}

export function McpCard({
  server,
  latestResult,
  testing = false,
  onEdit,
  onTest,
  onDelete,
  onViewResult,
}: McpCardProps) {
  return (
    <div className="rounded-3xl border border-muted/60 bg-background p-5 space-y-4 shadow-sm">
      <div className="flex items-start justify-between gap-3">
        <div className="space-y-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <p className="text-sm font-semibold truncate">{server.display_name}</p>
            <span className="text-[10px] font-mono rounded-full bg-muted px-2 py-0.5 uppercase">
              {server.transport.kind}
            </span>
          </div>
          <p className="text-[11px] text-muted-foreground">
            timeout {server.timeout_ms} ms
          </p>
        </div>
        <Badge className={cn("text-[10px] uppercase", getStatusTone(server.status))}>
          {server.status}
        </Badge>
      </div>

      <div className="flex items-center justify-between text-[11px] text-muted-foreground">
        <span>{server.enabled ? "已启用" : "已禁用"}</span>
        <span>{server.discovered_tool_count} tools</span>
      </div>

      {server.last_error && (
        <div className="rounded-2xl bg-rose-500/5 border border-rose-500/20 px-3 py-2 text-[11px] text-rose-700">
          {server.last_error}
        </div>
      )}

      {latestResult && (
        <button
          type="button"
          className="w-full rounded-2xl border border-muted/60 bg-muted/20 px-3 py-3 space-y-1 text-left transition-colors hover:bg-muted/30"
          onClick={onViewResult}
        >
          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-semibold">{latestResult.message}</p>
            <span className="text-[10px] text-muted-foreground">{latestResult.latency_ms} ms</span>
          </div>
          <p className="text-[10px] text-muted-foreground">
            最近测试发现 {latestResult.discovered_tools.length} 个 tools
          </p>
        </button>
      )}

      <div className="flex items-center gap-2">
        <Button size="sm" variant="outline" className="flex-1" onClick={onEdit}>
          <Pencil className="h-4 w-4 mr-1.5" />
          编辑
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="flex-1"
          disabled={testing}
          onClick={onTest}
        >
          {testing ? <Activity className="h-4 w-4 mr-1.5 animate-spin" /> : <Activity className="h-4 w-4 mr-1.5" />}
          测试连接
        </Button>
        <Button
          size="icon"
          variant="ghost"
          className="text-muted-foreground hover:text-destructive"
          onClick={onDelete}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  )
}
