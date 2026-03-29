"use client";

import * as React from "react";
import { Activity, Cpu, HardDrive, Radar, TimerReset, Workflow } from "lucide-react";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { ThreadPoolSnapshot } from "@/lib/types/chat";

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

interface MetricCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail?: string;
  tone?: "default" | "warning" | "subtle";
}

function MetricCard({ icon, label, value, detail, tone = "default" }: MetricCardProps) {
  return (
    <Card
      className={cn(
        "border-muted/60 bg-background/70 backdrop-blur-xl shadow-[0_8px_24px_rgba(0,0,0,0.06)]",
        tone === "warning" && "border-amber-300/60 bg-amber-50/40 dark:bg-amber-950/20",
        tone === "subtle" && "bg-muted/30",
      )}
    >
      <CardHeader className="flex-row items-start justify-between gap-3 space-y-0 pb-2">
        <div className="space-y-1">
          <CardDescription className="text-xs font-medium uppercase tracking-[0.2em]">
            {label}
          </CardDescription>
          <CardTitle className="text-2xl font-semibold tracking-tight">
            {value}
          </CardTitle>
        </div>
        <Badge variant="outline" className="rounded-full px-2.5 py-0.5">
          {icon}
        </Badge>
      </CardHeader>
      {detail ? (
        <CardContent className="pt-0 text-xs text-muted-foreground">
          {detail}
        </CardContent>
      ) : null}
    </Card>
  );
}

interface ThreadMonitorSummaryProps {
  snapshot: ThreadPoolSnapshot | null;
  observedCount: number;
  loading: boolean;
}

export function ThreadMonitorSummary({
  snapshot,
  observedCount,
  loading,
}: ThreadMonitorSummaryProps) {
  const capturedAt = snapshot?.captured_at
    ? new Date(snapshot.captured_at).toLocaleString("zh-CN", {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "尚未刷新";

  return (
    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      <MetricCard
        icon={<Workflow className="size-3.5" />}
        label="活跃线程"
        value={
          snapshot ? `${snapshot.active_threads} / ${snapshot.max_threads}` : "—"
        }
        detail="当前已装载到内存中的线程数"
      />
      <MetricCard
        icon={<Activity className="size-3.5" />}
        label="运行中"
        value={snapshot ? `${snapshot.running_threads}` : "—"}
        detail={
          snapshot
            ? `${snapshot.queued_jobs} 个任务排队，${snapshot.cooling_threads} 个线程冷却中`
            : "等待池快照"
        }
      />
      <MetricCard
        icon={<TimerReset className="size-3.5" />}
        label="最近观测"
        value={loading ? "刷新中" : `${observedCount}`}
        detail={`快照更新时间 ${capturedAt}`}
        tone={loading ? "warning" : "subtle"}
      />
      <MetricCard
        icon={<HardDrive className="size-3.5" />}
        label="估算内存"
        value={snapshot ? formatBytes(snapshot.estimated_memory_bytes) : "—"}
        detail={
          snapshot
            ? `峰值 ${formatBytes(snapshot.peak_estimated_memory_bytes)}`
            : "来自池级快照"
        }
      />
      <MetricCard
        icon={<Cpu className="size-3.5" />}
        label="进程内存"
        value={
          snapshot?.process_memory_bytes != null
            ? formatBytes(snapshot.process_memory_bytes)
            : "—"
        }
        detail={
          snapshot?.peak_process_memory_bytes != null
            ? `峰值 ${formatBytes(snapshot.peak_process_memory_bytes)}`
            : "暂未提供"
        }
      />
      <MetricCard
        icon={<Radar className="size-3.5" />}
        label="冷却 / 驱逐"
        value={snapshot ? `${snapshot.cooling_threads} / ${snapshot.evicted_threads}` : "—"}
        detail="帮助判断池是否需要回收或扩容"
      />
    </div>
  );
}
