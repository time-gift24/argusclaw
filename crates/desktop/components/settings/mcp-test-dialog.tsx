"use client"

import { CheckCircle2, CircleAlert, Loader2, Network, Wrench } from "lucide-react"

import {
  type McpConnectionTestResult,
  type McpDiscoveredToolRecord,
} from "@/lib/tauri"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { cn } from "@/lib/utils"

interface McpTestDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  serverName: string
  result: McpConnectionTestResult | null
  discoveredTools: McpDiscoveredToolRecord[]
  testing?: boolean
  onRetest: () => void
}

function getStatusTone(status: McpConnectionTestResult["status"]) {
  switch (status) {
    case "ready":
      return "border-emerald-200 text-emerald-700 bg-emerald-50/70"
    case "failed":
      return "border-rose-200 text-rose-700 bg-rose-50/70"
    case "disabled":
      return "border-slate-200 text-slate-600 bg-slate-50/70"
    default:
      return "border-amber-200 text-amber-700 bg-amber-50/70"
  }
}

export function McpTestDialog({
  open,
  onOpenChange,
  serverName,
  result,
  discoveredTools,
  testing = false,
  onRetest,
}: McpTestDialogProps) {
  const statusLabel = testing
    ? "正在测试"
    : result?.status === "ready"
      ? "连接成功"
      : "连接失败"

  const statusTone = testing
    ? "border-sky-200 text-sky-700 bg-sky-50/70"
    : result
      ? getStatusTone(result.status)
      : "border-muted/60 text-muted-foreground bg-muted/20"

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Network className="h-4 w-4 text-primary" />
            MCP 测试结果
          </DialogTitle>
          <DialogDescription>
            {serverName || "未命名 MCP Server"} 的连接结果与最近发现到的 tools 快照。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-5">
          <div className="flex items-center justify-between gap-3 rounded-2xl border border-muted/60 bg-muted/20 px-4 py-3">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <Badge variant="outline" className={cn("text-[10px] uppercase", statusTone)}>
                  {testing ? (
                    <Loader2 className="mr-1.5 h-3 w-3 animate-spin" />
                  ) : result?.status === "ready" ? (
                    <CheckCircle2 className="mr-1.5 h-3 w-3" />
                  ) : (
                    <CircleAlert className="mr-1.5 h-3 w-3" />
                  )}
                  {statusLabel}
                </Badge>
                {result && (
                  <span className="text-[11px] text-muted-foreground">
                    {result.latency_ms} ms
                  </span>
                )}
              </div>
              <p className="text-sm font-semibold">
                {testing
                  ? "正在尝试连接 MCP server..."
                  : result?.message ?? "点击重新测试后可查看详细结果。"}
              </p>
              {result && (
                <p className="text-[11px] text-muted-foreground">
                  checked_at: {result.checked_at}
                </p>
              )}
            </div>
          </div>

          <div className="space-y-3">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground">
              <Wrench className="h-4 w-4 text-primary" />
              Discovered Tools
            </div>
            {discoveredTools.length === 0 ? (
              <div className="rounded-2xl border border-dashed border-muted/60 px-4 py-6 text-sm text-muted-foreground">
                当前没有 discovery 快照。请检查 transport 配置或再次测试连接。
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 max-h-[320px] overflow-y-auto pr-1 custom-scrollbar">
                {discoveredTools.map((tool) => (
                  <div
                    key={`${tool.server_id}-${tool.tool_name_original}`}
                    className="rounded-2xl border border-muted/60 bg-background px-4 py-3 space-y-1"
                  >
                    <p className="text-sm font-semibold truncate">{tool.tool_name_original}</p>
                    <p className="text-[11px] text-muted-foreground line-clamp-3">
                      {tool.description || "No description"}
                    </p>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        <DialogFooter showCloseButton>
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
          <Button size="sm" onClick={onRetest} disabled={testing}>
            {testing ? (
              <>
                <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" />
                测试中...
              </>
            ) : (
              "重新测试"
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
