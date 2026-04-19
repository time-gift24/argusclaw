"use client";

import type { FC } from "react";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import type { JobDetailPayload, JobDetailStatus } from "@/lib/types/chat";
import { cn } from "@/lib/utils";

function formatTimestamp(value: string | null | undefined) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString("zh-CN", {
    hour12: false,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function statusLabel(status: JobDetailStatus) {
  switch (status) {
    case "running":
      return "运行中";
    case "completed":
      return "已完成";
    case "failed":
      return "失败";
    case "cancelled":
      return "已取消";
  }
}

function statusBadgeClass(status: JobDetailStatus) {
  switch (status) {
    case "running":
      return "border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300";
    case "completed":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "failed":
      return "border-destructive/30 bg-destructive/10 text-destructive";
    case "cancelled":
      return "border-muted-foreground/30 bg-muted/40 text-muted-foreground";
  }
}

export const SubagentJobDetailsPanel: FC<{
  detail: JobDetailPayload | null;
}> = ({ detail }) => {
  if (!detail) return null;

  const outputLabel = detail?.result_text
    ? "最终产出"
    : detail?.summary_text
      ? "结果摘要"
      : "暂无详细结果";
  const outputBody =
    detail?.result_text ??
    detail?.summary_text ??
    "任务已结束，但详细结果暂不可用";

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="border-b border-border/60 px-5 py-4">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="truncate text-sm font-semibold text-foreground">
                {detail.agent_display_name}
              </h2>
              <Badge
                variant="outline"
                className={cn(
                  "rounded-full px-2.5 py-0.5 text-[10px] uppercase tracking-[0.18em]",
                  statusBadgeClass(detail.status),
                )}
              >
                {statusLabel(detail.status)}
              </Badge>
            </div>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {detail.agent_description || "后台子 agent 任务"}
            </p>
          </div>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-5 py-5 custom-scrollbar">
        <section className="space-y-3">
          <div className="flex items-center justify-between gap-3">
            <h3 className="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
              {outputLabel}
            </h3>
            <span className="text-[11px] text-muted-foreground">
              Job {detail.job_id}
            </span>
          </div>
          <div className="rounded-2xl border border-border/60 bg-muted/20 px-4 py-3">
            <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-foreground/90">
              {outputBody}
            </div>
          </div>
        </section>

        <section className="space-y-3">
          <h3 className="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
            任务信息
          </h3>
          <dl className="grid gap-3 rounded-2xl border border-border/60 bg-background/80 p-4 sm:grid-cols-2">
            <div className="space-y-1">
              <dt className="text-[11px] uppercase tracking-wider text-muted-foreground">
                开始时间
              </dt>
              <dd className="text-sm text-foreground">
                {formatTimestamp(detail.started_at)}
              </dd>
            </div>
            <div className="space-y-1">
              <dt className="text-[11px] uppercase tracking-wider text-muted-foreground">
                完成时间
              </dt>
              <dd className="text-sm text-foreground">
                {formatTimestamp(detail.finished_at)}
              </dd>
            </div>
            <div className="space-y-1">
              <dt className="text-[11px] uppercase tracking-wider text-muted-foreground">
                输入 Tokens
              </dt>
              <dd className="text-sm text-foreground">
                {detail.input_tokens ?? "—"}
              </dd>
            </div>
            <div className="space-y-1">
              <dt className="text-[11px] uppercase tracking-wider text-muted-foreground">
                输出 Tokens
              </dt>
              <dd className="text-sm text-foreground">
                {detail.output_tokens ?? "—"}
              </dd>
            </div>
          </dl>
          <div className="rounded-2xl border border-border/60 bg-muted/15 px-4 py-3">
            <div className="mb-2 text-[11px] font-bold uppercase tracking-wider text-muted-foreground">
              原始 Prompt
            </div>
            <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-foreground/85">
              {detail.prompt || "—"}
            </div>
          </div>
        </section>

        <section className="space-y-3">
          <h3 className="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
            执行过程
          </h3>
          <div className="space-y-3 rounded-2xl border border-border/60 bg-background/80 p-4">
            {detail.timeline.length > 0 ? (
              detail.timeline.map((entry, index) => (
                <div
                  key={`${entry.kind}-${entry.at}-${index}`}
                  className="flex gap-3"
                >
                  <div className="mt-1 flex flex-col items-center">
                    <div
                      className={cn(
                        "size-2.5 rounded-full",
                        entry.status === "failed"
                          ? "bg-destructive"
                          : entry.status === "cancelled"
                            ? "bg-muted-foreground"
                          : entry.status === "completed"
                            ? "bg-emerald-500"
                            : "bg-sky-500",
                      )}
                    />
                    {index < detail.timeline.length - 1 && (
                      <div className="mt-1 h-8 w-px bg-border/70" />
                    )}
                  </div>
                  <div className="min-w-0 flex-1 pb-1">
                    <div className="text-sm font-medium text-foreground">
                      {entry.label}
                    </div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {formatTimestamp(entry.at)}
                    </div>
                    {entry.reason && (
                      <div className="mt-1 text-xs text-muted-foreground">
                        原因：{entry.reason}
                      </div>
                    )}
                  </div>
                </div>
              ))
            ) : (
              <div className="text-sm text-muted-foreground">
                暂未记录执行过程
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
};

export const SubagentJobDetailsDrawer: FC = () => {
  const session = useActiveChatSession();
  const closeJobDetails = useChatStore((state) => state.closeJobDetails);

  const selectedJobId = session?.selectedJobDetailId ?? null;
  const detail =
    selectedJobId && session
      ? (session.jobDetails[selectedJobId] ?? null)
      : null;
  const open = !!detail;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => !nextOpen && closeJobDetails()}
    >
      <DialogContent
        showCloseButton
        className="left-auto right-0 top-0 h-dvh w-full max-w-none translate-x-0 translate-y-0 gap-0 rounded-none border-l border-border/60 p-0 text-sm shadow-2xl sm:w-[40rem] sm:max-w-[40rem]"
      >
        <SubagentJobDetailsPanel detail={detail} />
      </DialogContent>
    </Dialog>
  );
};
