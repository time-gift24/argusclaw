"use client";

import { Loader2, StopCircle } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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

function kindLabel(kind: ThreadPoolThreadState["kind"]): string {
  return kind === "chat" ? "Chat" : "Job";
}

interface ThreadMonitorTableProps {
  threads: ThreadPoolThreadState[];
  stoppingJobIds: Record<string, true>;
  onStopJob: (jobId: string) => void | Promise<void>;
}

export function ThreadMonitorTable({
  threads,
  stoppingJobIds,
  onStopJob,
}: ThreadMonitorTableProps) {
  return (
    <Card className="border-muted/60 bg-background/70 backdrop-blur-xl shadow-[0_8px_24px_rgba(0,0,0,0.06)]">
      <CardHeader className="pb-3">
        <CardTitle className="text-base">统一 Runtime 列表</CardTitle>
        <CardDescription>
          展示后端当前返回的 chat / job runtime 状态，按最近活动时间排序，并支持停止运行中的 job。
        </CardDescription>
      </CardHeader>
      <CardContent className="pt-0">
        {threads.length === 0 ? (
          <div className="flex min-h-56 items-center justify-center rounded-xl border border-dashed border-muted-foreground/20 bg-muted/20 px-6 text-center text-sm text-muted-foreground">
            当前筛选条件下没有 runtime 数据，刷新快照或切换筛选条件后会更新。
          </div>
        ) : (
          <div className="overflow-x-auto rounded-xl border border-muted-foreground/10">
            <div className="min-w-[1180px] grid grid-cols-[1.2fr_0.7fr_1fr_0.9fr_0.8fr_1fr_0.8fr_1fr] gap-0 border-b border-muted-foreground/10 bg-muted/30 px-4 py-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
              <div>Thread</div>
              <div>Kind</div>
              <div>Job</div>
              <div>Status</div>
              <div>Memory</div>
              <div>Last Active</div>
              <div>Recovery</div>
              <div>操作</div>
            </div>
            <div className="min-w-[1180px] divide-y divide-muted-foreground/10">
              {threads.map((thread) => {
                const canStop =
                  thread.kind === "job" &&
                  thread.jobId &&
                  (thread.status === "queued" || thread.status === "running");
                const isStopping = thread.jobId
                  ? !!stoppingJobIds[thread.jobId]
                  : false;

                return (
                  <div
                    key={thread.threadId}
                    className={cn(
                      "grid grid-cols-[1.2fr_0.7fr_1fr_0.9fr_0.8fr_1fr_0.8fr_1fr] items-center gap-0 px-4 py-3 text-sm",
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
                    <div>
                      <Badge variant="outline" className="rounded-full">
                        {kindLabel(thread.kind)}
                      </Badge>
                    </div>
                    <div className="min-w-0 text-sm text-foreground/90">
                      {thread.jobId ?? thread.sessionId ?? "—"}
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
                    <div>
                      {canStop && thread.jobId ? (
                        <Button
                          type="button"
                          variant="outline"
                          className={cn(
                            "h-11 min-w-28 rounded-xl border px-4 text-sm",
                            isStopping
                              ? "border-amber-500/30 bg-amber-500/10 text-amber-700 hover:bg-amber-500/10 dark:text-amber-300"
                              : "border-destructive/30 bg-destructive/5 text-destructive hover:bg-destructive/10",
                          )}
                          disabled={isStopping}
                          aria-label={isStopping ? "正在停止任务" : "停止任务"}
                          onClick={() => void onStopJob(thread.jobId!)}
                        >
                          {isStopping ? (
                            <>
                              <Loader2 className="mr-2 size-4 animate-spin" />
                              正在停止
                            </>
                          ) : (
                            <>
                              <StopCircle className="mr-2 size-4" />
                              停止任务
                            </>
                          )}
                        </Button>
                      ) : (
                        <span className="text-sm text-muted-foreground">—</span>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
