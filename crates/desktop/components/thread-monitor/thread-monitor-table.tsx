"use client";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import type { ThreadPoolThreadState } from "@/lib/chat-store";

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";

  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }

  return `${size >= 10 || unitIndex === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unitIndex]}`;
}

function formatTime(value: string | null): string {
  if (!value) return "—";
  return new Date(value).toLocaleString("zh-CN", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function statusLabel(status: ThreadPoolThreadState["status"]): string {
  switch (status) {
    case "queued":
      return "排队中";
    case "running":
      return "运行中";
    case "cooling":
      return "冷却中";
    case "evicted":
      return "已驱逐";
    case "loading":
      return "加载中";
    case "inactive":
      return "未激活";
  }
}

function statusBadgeClass(status: ThreadPoolThreadState["status"]): string {
  switch (status) {
    case "running":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "queued":
      return "border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300";
    case "cooling":
      return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "evicted":
      return "border-destructive/30 bg-destructive/10 text-destructive";
    case "loading":
      return "border-muted-foreground/30 bg-muted/50 text-muted-foreground";
    case "inactive":
      return "border-muted-foreground/30 bg-muted/50 text-muted-foreground";
  }
}

function reasonLabel(reason: ThreadPoolThreadState["lastReason"]): string {
  if (!reason) return "—";
  switch (reason) {
    case "cooling_expired":
      return "冷却到期";
    case "memory_pressure":
      return "内存压力";
    case "cancelled":
      return "已取消";
    case "execution_failed":
      return "执行失败";
  }
}

interface ThreadMonitorTableProps {
  threads: ThreadPoolThreadState[];
}

export function ThreadMonitorTable({ threads }: ThreadMonitorTableProps) {
  return (
    <Card className="border-muted/60 bg-background/70 backdrop-blur-xl shadow-[0_8px_24px_rgba(0,0,0,0.06)]">
      <CardHeader className="pb-3">
        <CardTitle className="text-base">最近观测线程</CardTitle>
        <CardDescription>
          仅展示最近通过线程池事件捕获到的线程，按最新活动时间排序。
        </CardDescription>
      </CardHeader>
      <CardContent className="pt-0">
        {threads.length === 0 ? (
          <div className="flex min-h-56 items-center justify-center rounded-xl border border-dashed border-muted-foreground/20 bg-muted/20 px-6 text-center text-sm text-muted-foreground">
            暂无线程观测数据，切到 Threads 页签或等待后台任务产生事件后会自动出现。
          </div>
        ) : (
          <div className="overflow-x-auto rounded-xl border border-muted-foreground/10">
            <div className="min-w-[920px] grid grid-cols-[1.3fr_1fr_0.9fr_0.8fr_1fr_0.8fr] gap-0 border-b border-muted-foreground/10 bg-muted/30 px-4 py-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
              <div>Thread</div>
              <div>Job</div>
              <div>Status</div>
              <div>Memory</div>
              <div>Last Active</div>
              <div>Recovery</div>
            </div>
            <div className="min-w-[920px] divide-y divide-muted-foreground/10">
              {threads.map((thread) => (
                <div
                  key={thread.threadId}
                  className={cn(
                    "grid grid-cols-[1.3fr_1fr_0.9fr_0.8fr_1fr_0.8fr] items-center gap-0 px-4 py-3 text-sm",
                    "hover:bg-muted/20",
                  )}
                >
                  <div className="min-w-0">
                    <div className="font-mono text-xs text-foreground/90">
                      {thread.threadId}
                    </div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {thread.eventCount} 次事件 · {reasonLabel(thread.lastReason)}
                    </div>
                  </div>
                  <div className="min-w-0 text-sm text-foreground/90">
                    {thread.jobId ?? "—"}
                  </div>
                  <div>
                    <Badge variant="outline" className={cn("rounded-full", statusBadgeClass(thread.status))}>
                      {statusLabel(thread.status)}
                    </Badge>
                  </div>
                  <div className="text-sm text-foreground/90">
                    {formatBytes(thread.estimatedMemoryBytes)}
                  </div>
                  <div className="text-sm text-muted-foreground">
                    {formatTime(thread.lastActiveAt)}
                  </div>
                  <div className="text-sm">
                    <Badge variant={thread.recoverable ? "secondary" : "destructive"}>
                      {thread.recoverable ? "可恢复" : "不可恢复"}
                    </Badge>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
