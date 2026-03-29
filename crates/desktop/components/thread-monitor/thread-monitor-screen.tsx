"use client";

import * as React from "react";
import { Loader2, RefreshCw } from "lucide-react";

import { ThreadMonitorSummary } from "@/components/thread-monitor/thread-monitor-summary";
import { ThreadMonitorTable } from "@/components/thread-monitor/thread-monitor-table";
import { Badge } from "@/components/ui/badge";
import { useChatStore } from "@/lib/chat-store";

const THREAD_POOL_POLL_INTERVAL_MS = 5_000;

export function ThreadMonitorScreen() {
  const snapshot = useChatStore((state) => state.threadPoolSnapshot);
  const threads = useChatStore((state) => state.threadPoolThreads);
  const loading = useChatStore((state) => state.threadPoolSnapshotLoading);
  const error = useChatStore((state) => state.threadPoolError);
  const refresh = useChatStore((state) => state.refreshThreadPoolSnapshot);

  React.useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => {
      void refresh();
    }, THREAD_POOL_POLL_INTERVAL_MS);

    return () => {
      window.clearInterval(timer);
    };
  }, [refresh]);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto flex w-full max-w-(--thread-max-width) flex-1 flex-col gap-4 px-4 pb-6 pt-2">
        <div className="flex flex-col gap-3 rounded-3xl border border-muted/60 bg-gradient-to-br from-background/80 via-background/70 to-muted/30 p-5 shadow-[0_16px_48px_rgba(0,0,0,0.08)] backdrop-blur-2xl">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <h2 className="text-xl font-semibold tracking-tight">Thread Monitor</h2>
                <Badge variant="outline" className="rounded-full">
                  只读
                </Badge>
              </div>
              <p className="max-w-2xl text-sm text-muted-foreground">
                监控线程池的总览和最近观测到的线程状态。页面只读，线程列表来自事件流，池级指标来自快照拉取。
              </p>
            </div>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {loading ? <Loader2 className="size-4 animate-spin text-primary" /> : <RefreshCw className="size-4 text-muted-foreground/70" />}
              <span>{loading ? "正在刷新监控快照" : "已连接线程池监控"}</span>
            </div>
          </div>
          {error ? (
            <div className="rounded-xl border border-amber-500/20 bg-amber-500/10 px-4 py-3 text-sm text-amber-700 dark:text-amber-300">
              {error}
            </div>
          ) : null}
        </div>

        <ThreadMonitorSummary
          snapshot={snapshot}
          observedCount={threads.length}
          loading={loading}
        />

        <ThreadMonitorTable threads={threads} />
      </div>
    </div>
  );
}
