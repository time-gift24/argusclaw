import {
  ComposerAttachments,
  UserMessageAttachments,
} from "@/components/assistant-ui/attachment";
import { MarkdownText } from "@/components/assistant-ui/markdown-text";
import { ToolFallbackImpl } from "@/components/assistant-ui/tool-fallback";
import { TooltipIconButton } from "@/components/assistant-ui/tooltip-icon-button";
import { TokenRing } from "@/components/token-ring";
import { AgentSelector } from "@/components/assistant-ui/agent-selector";
import { ProviderSelector } from "@/components/assistant-ui/provider-selector";
import {
  NewSessionButton,
  SessionHistoryButton,
} from "@/components/assistant-ui/session-selector";
import { SubagentJobDetailsDrawer } from "@/components/assistant-ui/subagent-job-details-drawer";
import { ChatStatusBanner } from "@/components/chat/chat-status-banner";
import { PlanPanel } from "@/components/chat/plan-panel";
import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";
import type { ChatStore, PendingToolCall } from "@/lib/chat-store";
import { providers } from "@/lib/tauri";
import { Badge } from "@/components/ui/badge";
import type { ChatMessagePayload } from "@/lib/types/chat";
import { Button } from "@/components/ui/button";
import { useToast } from "@/components/ui/toast";
import { cn } from "@/lib/utils";
import {
  ActionBarMorePrimitive,
  ActionBarPrimitive,
  AuiIf,
  BranchPickerPrimitive,
  ComposerPrimitive,
  ErrorPrimitive,
  MessagePrimitive,
  SuggestionPrimitive,
  ThreadPrimitive,
  useAui,
  useAuiState,
} from "@assistant-ui/react";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  CopyIcon,
  DownloadIcon,
  MoreHorizontalIcon,
  PencilIcon,
  RefreshCwIcon,
  Loader2,
  StopCircle,
  Bot,
  Sparkles,
  CircleAlert,
  Wrench,
  ChevronDown,
} from "lucide-react";
import type { FC } from "react";
import { useEffect, useRef } from "react";

const ComposerAction: FC = () => {
  const session = useActiveChatSession();
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const isCompacting = session?.status === "compacting";
  const aui = useAui();

  // Fetch context window
  useEffect(() => {
    if (!session?.effectiveProviderId || session.contextWindow !== null) return;
    providers.getContextWindow(session.effectiveProviderId).then((cw) => {
      useChatStore.setState((s: ChatStore) => ({
        sessionsByKey: {
          ...s.sessionsByKey,
          [s.activeSessionKey!]: {
            ...s.sessionsByKey[s.activeSessionKey!],
            contextWindow: cw,
          },
        },
      }));
    });
  }, [session?.effectiveProviderId, session?.contextWindow]);

  const handleCancel = () => {
    void useChatStore.getState().cancelTurn();
    try {
      aui.thread().cancelRun();
    } catch (error) {
      // External store runtime may not implement cancelRun; backend cancelTurn is authoritative.
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      if (!message.includes("does not support cancelling runs")) {
        console.error("取消运行失败:", error);
      }
    }
  };

  return (
    <div className="aui-composer-action-wrapper relative mx-2 mb-2 flex items-center justify-between gap-2">
      <div className="flex items-center gap-1.5 pl-1">
        <NewSessionButton />
        <SessionHistoryButton />
        <AgentSelector />
        <ProviderSelector />
      </div>
      <div className="flex items-center gap-2 pr-1">
        {session && session.tokenCount > 0 && session.contextWindow && (
          <TokenRing
            modelContextWindow={session.contextWindow}
            tokenCount={session.tokenCount}
            className="size-8 opacity-80"
          />
        )}

        {isRunning ? (
          <Button
            variant="ghost"
            size="icon"
            className="size-8 rounded-full text-destructive hover:bg-destructive/10"
            aria-label="Stop generation"
            onClick={handleCancel}
          >
            <StopCircle className="size-5" />
          </Button>
        ) : (
          <ComposerPrimitive.Send asChild>
            <TooltipIconButton
              tooltip={isCompacting ? "上下文压缩中" : "发送消息"}
              side="top"
              type="button"
              variant="default"
              size="icon"
              className="aui-composer-send size-8 rounded-full shadow-lg shadow-primary/20 transition-all active:scale-95"
              aria-label="Send message"
              disabled={isCompacting}
            >
              <ArrowUpIcon className="aui-composer-send-icon size-4" />
            </TooltipIconButton>
          </ComposerPrimitive.Send>
        )}
      </div>
    </div>
  );
};

const Composer: FC = () => {
  const session = useActiveChatSession();
  const isCompacting = session?.status === "compacting";

  return (
    <ComposerPrimitive.Root className="aui-composer-root relative flex w-full flex-col">
      <ComposerPrimitive.AttachmentDropzone className="aui-composer-attachment-dropzone flex w-full flex-col rounded-[24px] border border-muted/60 bg-background/80 backdrop-blur-2xl px-1 pt-2 shadow-[0_8px_32px_0_rgba(0,0,0,0.15)] shadow-primary/10 transition-all duration-300 has-[textarea:focus-visible]:border-primary/40 has-[textarea:focus-visible]:ring-4 has-[textarea:focus-visible]:ring-primary/5 data-[dragging=true]:border-primary data-[dragging=true]:border-dashed data-[dragging=true]:bg-primary/5">
        <ComposerAttachments />
        <ComposerPrimitive.Input
          placeholder="给 ArgusWing 发送消息..."
          className="aui-composer-input mb-1 max-h-48 min-h-14 w-full resize-none bg-transparent px-5 pt-3 pb-4 text-sm leading-relaxed outline-none placeholder:text-muted-foreground/50 focus-visible:ring-0 custom-scrollbar"
          rows={1}
          autoFocus
          disabled={isCompacting}
          aria-label="Message input"
        />
        <ComposerAction />
      </ComposerPrimitive.AttachmentDropzone>
    </ComposerPrimitive.Root>
  );
};

const isFoldedCompactionMessage = (message: ChatMessagePayload) =>
  !!message.metadata?.synthetic &&
  !!message.metadata?.collapsed_by_default &&
  ["compaction_prompt", "compaction_summary", "compaction_replay"].includes(
    message.metadata.mode ?? "",
  );

type ToolCallDisplayStatus =
  | { type: "complete" }
  | { type: "running" }
  | { type: "incomplete"; reason: "cancelled" | "error"; error?: unknown };

type RenderableToolCall = {
  toolCallId: string;
  toolName: string;
  argsText: string;
  result?: unknown;
  status: ToolCallDisplayStatus;
};

type RenderableTurnArtifacts = {
  reasoning: string;
  toolCalls: readonly RenderableToolCall[];
};

const ManualToolFallback = ToolFallbackImpl as (props: {
  toolName: string;
  argsText: string;
  result: unknown;
  status: ToolCallDisplayStatus;
}) => React.ReactElement;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const toManualToolStatus = (
  status: "streaming" | "running" | "completed",
): ToolCallDisplayStatus =>
  status === "completed"
    ? ({ type: "complete" } as const)
    : status === "running"
      ? ({ type: "running" } as const)
      : ({ type: "incomplete", reason: "cancelled" } as const);

const toSettledToolStatus = (value: Record<string, unknown>): ToolCallDisplayStatus =>
  value.isError === true
    ? ({ type: "incomplete", reason: "error", error: value.result } as const)
    : ({ type: "complete" } as const);

const toRenderablePendingToolCall = (
  toolCall: PendingToolCall,
): RenderableToolCall => ({
  toolCallId: toolCall.tool_call_id,
  toolName: toolCall.tool_name,
  argsText: toolCall.arguments_text,
  result: toolCall.result,
  status: toManualToolStatus(toolCall.status),
});

const toRenderableStoredToolCall = (
  value: unknown,
): RenderableToolCall | null => {
  if (!isRecord(value)) return null;
  if (
    typeof value.toolCallId !== "string" ||
    typeof value.toolName !== "string" ||
    typeof value.argsText !== "string"
  ) {
    return null;
  }

  return {
    toolCallId: value.toolCallId,
    toolName: value.toolName,
    argsText: value.argsText,
    result: value.result,
    status: toSettledToolStatus(value),
  };
};

const readTurnArtifacts = (value: unknown): RenderableTurnArtifacts | null => {
  if (!isRecord(value)) return null;

  const reasoning = typeof value.reasoning === "string" ? value.reasoning : "";
  const toolCalls = Array.isArray(value.toolCalls)
    ? value.toolCalls
        .map((toolCall) => toRenderableStoredToolCall(toolCall))
        .filter((toolCall): toolCall is RenderableToolCall => toolCall !== null)
    : [];

  if (reasoning.trim().length === 0 && toolCalls.length === 0) return null;

  return {
    reasoning,
    toolCalls,
  };
};

const buildPendingTurnArtifacts = (
  pendingAssistant: NonNullable<ReturnType<typeof useActiveChatSession>>["pendingAssistant"],
): RenderableTurnArtifacts | null => {
  if (!pendingAssistant) return null;

  const reasoning = pendingAssistant.reasoning;
  const toolCalls = pendingAssistant.toolCalls.map((toolCall) =>
    toRenderablePendingToolCall(toolCall),
  );

  if (reasoning.trim().length === 0 && toolCalls.length === 0) return null;

  return {
    reasoning,
    toolCalls,
  };
};

const CompactionGroups: FC = () => {
  const session = useActiveChatSession();
  if (!session) return null;

  const groups: ChatMessagePayload[][] = [];
  let currentGroup: ChatMessagePayload[] = [];

  for (const message of session.messages) {
    if (isFoldedCompactionMessage(message)) {
      currentGroup.push(message);
      continue;
    }

    if (currentGroup.length > 0) {
      groups.push(currentGroup);
      currentGroup = [];
    }
  }

  if (currentGroup.length > 0) {
    groups.push(currentGroup);
  }

  if (groups.length === 0) return null;

  return (
    <div className="mx-auto flex w-full max-w-(--thread-max-width) flex-col gap-3 px-4 py-2">
      {groups.map((compactionGroup, index) => (
        <details
          key={`compaction-group-${index}`}
          className="rounded-2xl border border-muted/40 bg-muted/20 px-4 py-3 text-sm"
        >
          <summary className="flex cursor-pointer list-none items-center gap-2 text-muted-foreground [&::-webkit-details-marker]:hidden">
            <Sparkles className="size-4 text-primary/70" />
            <span className="font-medium">已压缩上下文</span>
            <ChevronDown className="ml-auto size-4 opacity-60" />
          </summary>
          <div className="mt-3 space-y-3 text-sm">
            {compactionGroup.map((message, messageIndex) => (
              <div
                key={`compaction-item-${index}-${messageIndex}`}
                className="rounded-xl border border-muted/40 bg-background/70 px-3 py-2"
              >
                <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {message.role}
                </div>
                <div className="whitespace-pre-wrap leading-relaxed text-foreground/90">
                  {message.content}
                </div>
              </div>
            ))}
          </div>
        </details>
      ))}
    </div>
  );
};

const ThreadWelcome: FC = () => {
  return (
    <div className="aui-thread-welcome-root mx-auto my-auto flex w-full max-w-(--thread-max-width) grow flex-col">
      <div className="aui-thread-welcome-center flex w-full grow flex-col items-center justify-center py-12">
        <div className="aui-thread-welcome-message flex size-full flex-col items-center justify-center px-4 text-center">
          <div className="bg-primary/10 p-4 rounded-[2rem] text-primary mb-6 animate-in zoom-in-50 duration-500">
            <Bot className="size-10" />
          </div>
          <h1 className="aui-thread-welcome-message-inner fade-in slide-in-from-bottom-4 animate-in fill-mode-both font-bold text-3xl tracking-tight duration-500">
            欢迎来到 ArgusWing
          </h1>
          <p className="aui-thread-welcome-message-inner fade-in slide-in-from-bottom-4 animate-in fill-mode-both text-muted-foreground text-lg mt-3 delay-150 duration-500">
            我是您的 AI 助手，今天有什么可以帮您的？
          </p>
        </div>
      </div>
      <div className="mt-auto px-4">
        <div className="flex items-center gap-2 mb-4 text-xs font-bold text-muted-foreground uppercase tracking-widest px-1">
          <Sparkles className="size-3" />
          快速开始
        </div>
        <ThreadSuggestions />
      </div>
    </div>
  );
};

const ThreadSuggestionItem: FC = () => {
  return (
    <div className="aui-thread-welcome-suggestion-display fade-in slide-in-from-bottom-2 @md:nth-[n+3]:block nth-[n+3]:hidden animate-in fill-mode-both duration-300">
      <SuggestionPrimitive.Trigger send asChild>
        <Button
          variant="ghost"
          className="aui-thread-welcome-suggestion h-auto w-full @md:flex-col flex-wrap items-start justify-start gap-1.5 rounded-[20px] border border-muted/60 bg-muted/10 px-5 py-4 text-left text-sm transition-all hover:bg-muted hover:border-primary/30 group"
        >
          <span className="aui-thread-welcome-suggestion-text-1 font-bold group-hover:text-primary transition-colors">
            <SuggestionPrimitive.Title />
          </span>
          <span className="aui-thread-welcome-suggestion-text-2 text-muted-foreground text-xs leading-relaxed line-clamp-2">
            <SuggestionPrimitive.Description />
          </span>
        </Button>
      </SuggestionPrimitive.Trigger>
    </div>
  );
};

const ThreadSuggestions: FC = () => {
  return (
    <div className="aui-thread-welcome-suggestions grid w-full @md:grid-cols-2 gap-3 pb-8">
      <ThreadPrimitive.Suggestions
        components={{
          Suggestion: ThreadSuggestionItem,
        }}
      />
    </div>
  );
};

const MessageError: FC = () => {
  return (
    <MessagePrimitive.Error>
      <ErrorPrimitive.Root className="aui-message-error-root mt-3 rounded-2xl border border-destructive/20 bg-destructive/5 p-4 text-destructive text-sm backdrop-blur-sm animate-in zoom-in-95 duration-200">
        <div className="flex items-start gap-3">
          <CircleAlert className="size-4 shrink-0 mt-0.5" />
          <div className="space-y-1">
            <p className="font-bold uppercase tracking-tight text-[10px] opacity-70">
              发生错误
            </p>
            <ErrorPrimitive.Message className="aui-message-error-message leading-relaxed" />
          </div>
        </div>
      </ErrorPrimitive.Root>
    </MessagePrimitive.Error>
  );
};

const ToolCallList = ({
  toolCalls,
}: {
  toolCalls: readonly RenderableToolCall[];
}) => {
  if (toolCalls.length === 0) return null;

  const hasRunningTool = toolCalls.some(
    (toolCall) => toolCall.status.type === "running",
  );

  return (
    <div className="rounded-xl border border-muted/40 bg-muted/20 px-3 py-2.5">
      <div className="flex items-center gap-2.5 text-muted-foreground">
        <div className="rounded-lg bg-primary/10 p-1.5 text-primary">
          <Wrench className="size-3.5" />
        </div>
        <span className="text-[11px] font-bold uppercase tracking-widest">
          工具调用
        </span>
        {hasRunningTool && (
          <Loader2 className="ml-auto size-3 animate-spin text-primary" />
        )}
      </div>
      <div className="mt-2 flex max-h-[min(18rem,35vh)] flex-col gap-1 overflow-y-auto custom-scrollbar border-l-2 border-muted/30 pl-4 pr-1 ml-4">
        {toolCalls.map((toolCall) => (
          <ManualToolFallback
            key={toolCall.toolCallId}
            toolName={toolCall.toolName}
            argsText={toolCall.argsText}
            result={toolCall.result}
            status={toolCall.status}
          />
        ))}
      </div>
    </div>
  );
};

const TurnReasoningBlock = ({
  reasoning,
  isRunning,
}: {
  reasoning: string;
  isRunning?: boolean;
}) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const handleScroll = () => {
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= 1;
      isAtBottomRef.current = atBottom;
    };

    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => el.removeEventListener("scroll", handleScroll);
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && isAtBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  });

  if (reasoning.trim().length === 0) return null;

  return (
    <div className="aui-reasoning-block mb-4 text-sm animate-in fade-in slide-in-from-top-1 duration-300">
      <details className="group w-full" open>
        <summary className="flex w-full cursor-pointer list-none items-center gap-2.5 rounded-xl bg-muted/30 px-3 py-2 text-muted-foreground transition-all hover:bg-muted/50 [&::-webkit-details-marker]:hidden border border-muted/40">
          {isRunning ? (
            <>
              <Loader2 className="size-3 animate-spin text-primary" />
              <span className="text-[11px] font-bold uppercase tracking-widest text-primary/80">
                思考中...
              </span>
            </>
          ) : (
            <>
              <div className="size-1.5 rounded-full bg-emerald-500/50" />
              <span className="text-[11px] font-bold uppercase tracking-widest opacity-70">
                思考完成
              </span>
            </>
          )}
          <ChevronDownIcon className="ml-auto size-3.5 shrink-0 opacity-40 transition-transform duration-300 group-open:rotate-180" />
        </summary>
        <div
          ref={scrollRef}
          className="max-h-[200px] overflow-y-auto mt-2 px-4 py-3 text-xs leading-relaxed text-muted-foreground/80 border-l-2 border-muted/40 ml-3 whitespace-pre-wrap break-words italic bg-muted/5 rounded-r-xl"
        >
          {reasoning}
        </div>
      </details>
    </div>
  );
};

const TurnArtifactsPanel = ({
  turnArtifacts,
  isRunning = false,
}: {
  turnArtifacts: RenderableTurnArtifacts | null;
  isRunning?: boolean;
}) => {
  if (!turnArtifacts) return null;

  return (
    <div className="flex flex-col gap-3">
      <TurnReasoningBlock
        reasoning={turnArtifacts.reasoning}
        isRunning={isRunning}
      />
      <ToolCallList toolCalls={turnArtifacts.toolCalls} />
    </div>
  );
};

const AssistantTurnArtifacts: FC = () => {
  const turnArtifacts = useAuiState((s) =>
    readTurnArtifacts(s.message.metadata.custom.turnArtifacts),
  );
  const isRunning = useAuiState((s) => s.message.status?.type === "running");

  if (!turnArtifacts) return null;

  return (
    <TurnArtifactsPanel turnArtifacts={turnArtifacts} isRunning={isRunning} />
  );
};

const PendingAssistantArtifacts: FC = () => {
  const session = useActiveChatSession();
  const pendingAssistant = session?.pendingAssistant;

  if (!pendingAssistant) return null;

  const hasPlan = !!pendingAssistant.plan && pendingAssistant.plan.length > 0;
  const retryState = pendingAssistant.retry;
  const turnArtifacts = buildPendingTurnArtifacts(pendingAssistant);

  if (!hasPlan && !retryState && !turnArtifacts) return null;

  return (
    <div className="mx-auto w-full max-w-(--thread-max-width) px-4 pb-2 flex flex-col gap-3">
      {retryState && (
        <div className="flex items-start gap-3 rounded-xl border border-amber-200/80 bg-amber-50/90 px-3 py-2.5 text-amber-900 shadow-sm">
          <div className="mt-0.5 rounded-full bg-amber-100 p-1 text-amber-700">
            <Loader2 className="size-3.5 animate-spin" />
          </div>
          <div className="min-w-0 flex-1 space-y-1">
            <div className="flex items-center gap-2">
              <span className="text-[11px] font-bold uppercase tracking-widest">
                正在重试请求
              </span>
              <Badge
                variant="secondary"
                className="border border-amber-200 bg-amber-100/80 text-[10px] text-amber-800"
              >
                {retryState.attempt}/{retryState.maxRetries}
              </Badge>
            </div>
            <p className="text-sm leading-relaxed text-amber-950/85">
              {retryState.error}
            </p>
          </div>
        </div>
      )}
      {hasPlan && <PlanPanel plan={pendingAssistant.plan!} />}
      <TurnArtifactsPanel turnArtifacts={turnArtifacts} isRunning />
    </div>
  );
};

const JobStatusArtifacts: FC = () => {
  const session = useActiveChatSession();
  const jobStatuses = Object.values(session?.jobStatuses ?? {});
  const stoppingJobIds = useChatStore((state) => state.stoppingJobIds);
  const threadPoolThreads = useChatStore((state) => state.threadPoolThreads);
  const stopJob = useChatStore((state) => state.stopJob);
  const openJobDetails = useChatStore((state) => state.openJobDetails);
  const { addToast } = useToast();
  const detailActionLabel = "查看详情";

  if (jobStatuses.length === 0) return null;

  const runtimeStatusByJobId = new Map(
    threadPoolThreads
      .filter((thread) => thread.kind === "job" && thread.jobId)
      .map((thread) => [thread.jobId!, thread.status]),
  );

  const sorted = [...jobStatuses].sort((left, right) => {
    const leftRuntimeStatus = runtimeStatusByJobId.get(left.job_id);
    const rightRuntimeStatus = runtimeStatusByJobId.get(right.job_id);
    const leftStatus = stoppingJobIds[left.job_id]
      ? "stopping"
      : leftRuntimeStatus === "queued"
        ? "queued"
        : left.status;
    const rightStatus = stoppingJobIds[right.job_id]
      ? "stopping"
      : rightRuntimeStatus === "queued"
        ? "queued"
        : right.status;

    const priority = (status: "stopping" | "queued" | typeof left.status) => {
      switch (status) {
        case "stopping":
          return 0;
        case "running":
          return 1;
        case "queued":
          return 2;
        case "failed":
          return 3;
        case "completed":
          return 4;
      }
    };

    const priorityDiff = priority(leftStatus) - priority(rightStatus);
    if (priorityDiff !== 0) return priorityDiff;
    return left.job_id.localeCompare(right.job_id);
  });

  const handleStopJob = async (jobId: string) => {
    try {
      await stopJob(jobId);
      addToast("success", "已发送停止请求");
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <div className="mx-auto w-full max-w-(--thread-max-width) px-4 pb-2">
      <details className="group/jobs w-full" open>
        <summary className="flex w-full cursor-pointer list-none items-center gap-2.5 rounded-xl border border-muted/40 bg-muted/20 px-3 py-2 text-muted-foreground transition-all hover:bg-muted/40 [&::-webkit-details-marker]:hidden">
          <div className="rounded-lg bg-primary/10 p-1.5 text-primary">
            <Bot className="size-3.5" />
          </div>
          <span className="text-[11px] font-bold uppercase tracking-widest">
            后台任务 {sorted.length} 个
          </span>
          <div className="ml-auto flex items-center gap-2">
            {sorted.some((job) => {
              const runtimeStatus = runtimeStatusByJobId.get(job.job_id);
              return (
                stoppingJobIds[job.job_id] ||
                runtimeStatus === "queued" ||
                job.status === "running"
              );
            }) && <Loader2 className="size-3 animate-spin text-primary" />}
            <ChevronDown className="size-3.5 opacity-40 transition-transform duration-300 group-open/jobs:rotate-180" />
          </div>
        </summary>

        <div className="mt-3 max-h-[min(22rem,40vh)] overflow-y-auto custom-scrollbar pr-1">
          <div className="flex flex-col gap-2">
            {sorted.map((job) => {
              const isStopping = !!stoppingJobIds[job.job_id];
              const runtimeStatus = runtimeStatusByJobId.get(job.job_id);
              const uiStatus = isStopping
                ? "stopping"
                : runtimeStatus === "queued"
                  ? "queued"
                  : job.status;
              const isRunning = uiStatus === "running";
              const isQueued = uiStatus === "queued";
              const isFailed = uiStatus === "failed";
              const isCompleted = uiStatus === "completed";
              const isActionable =
                (job.status === "running" || isQueued) && !isStopping;
              const detailAction = (
                <Button
                  type="button"
                  variant="outline"
                  className="h-11 rounded-xl border px-4 text-sm font-medium shadow-sm transition-all focus-visible:ring-2 focus-visible:ring-primary/40"
                  onClick={() => openJobDetails(job.job_id)}
                >
                  {detailActionLabel}
                </Button>
              );

              const statusLabel =
                uiStatus === "stopping"
                  ? "正在停止"
                  : uiStatus === "queued"
                    ? "排队中"
                    : uiStatus === "running"
                      ? "运行中"
                      : uiStatus === "failed"
                        ? "失败"
                        : "已完成";

              const statusBadgeClass =
                uiStatus === "stopping"
                  ? "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
                  : uiStatus === "queued"
                    ? "border-cyan-500/30 bg-cyan-500/10 text-cyan-700 dark:text-cyan-300"
                    : uiStatus === "running"
                      ? "border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300"
                      : uiStatus === "failed"
                        ? "border-destructive/30 bg-destructive/10 text-destructive"
                        : "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";

              return (
                <div
                  key={job.job_id}
                  className="rounded-2xl border border-muted/50 bg-background/85 px-4 py-4 text-sm shadow-sm transition-colors"
                >
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <button
                      type="button"
                      className="flex min-w-0 flex-1 items-start gap-3 text-left"
                      aria-label={`查看 ${job.agent_display_name ?? `Agent ${job.agent_id}`} 详情`}
                      onClick={() => openJobDetails(job.job_id)}
                    >
                      <div
                        className={cn(
                          "mt-0.5 rounded-xl p-2",
                          isStopping &&
                            "bg-amber-500/10 text-amber-700 dark:text-amber-300",
                          isQueued &&
                            "bg-cyan-500/10 text-cyan-700 dark:text-cyan-300",
                          isRunning && "bg-primary/10 text-primary",
                          isFailed && "bg-destructive/10 text-destructive",
                          isCompleted &&
                            "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
                        )}
                      >
                        {isStopping || isRunning || isQueued ? (
                          <Loader2 className="size-4 animate-spin" />
                        ) : isFailed ? (
                          <CircleAlert className="size-4" />
                        ) : (
                          <CheckIcon className="size-4" />
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="truncate font-semibold text-foreground">
                            {job.agent_display_name ?? `Agent ${job.agent_id}`}
                          </span>
                          <Badge
                            variant="outline"
                            className={cn(
                              "rounded-full px-2.5 py-0.5 text-[10px] uppercase tracking-[0.18em]",
                              statusBadgeClass,
                            )}
                          >
                            {statusLabel}
                          </Badge>
                        </div>
                        <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                          <span className="line-clamp-2 break-words">
                            {job.agent_description || "后台子 agent 任务"}
                          </span>
                        </div>
                        {job.prompt && (
                          <div className="mt-2 rounded-xl border border-muted/40 bg-muted/20 px-3 py-2 text-xs leading-relaxed text-foreground/80">
                            {job.prompt}
                          </div>
                        )}
                        {job.message && (
                          <div className="mt-2 whitespace-pre-wrap break-words rounded-xl border border-muted/40 bg-muted/35 px-3 py-2 text-xs leading-relaxed text-foreground/80">
                            {job.message}
                          </div>
                        )}
                      </div>
                    </button>
                    <div className="flex shrink-0 items-center sm:justify-end">
                      {isActionable || isStopping ? (
                        <div className="flex items-center gap-2">
                          {detailAction}
                          <Button
                            type="button"
                            variant="outline"
                            className={cn(
                              "h-11 min-w-28 rounded-xl border px-4 text-sm font-medium shadow-sm transition-all focus-visible:ring-2 focus-visible:ring-primary/40",
                              isStopping
                                ? "border-amber-500/30 bg-amber-500/10 text-amber-700 hover:bg-amber-500/10 dark:text-amber-300"
                                : "border-destructive/30 bg-destructive/5 text-destructive hover:bg-destructive/10",
                            )}
                            disabled={isStopping}
                            aria-label={
                              isStopping ? "正在停止任务" : "停止任务"
                            }
                            onClick={() => void handleStopJob(job.job_id)}
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
                        </div>
                      ) : (
                        detailAction
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </details>
    </div>
  );
};

const AssistantActionBar: FC = () => {
  return (
    <ActionBarPrimitive.Root
      hideWhenRunning
      autohide="not-last"
      className="aui-assistant-action-bar-root flex gap-1 text-muted-foreground"
    >
      <ActionBarPrimitive.Copy asChild>
        <TooltipIconButton tooltip="复制内容">
          <AuiIf condition={(s) => s.message.isCopied}>
            <CheckIcon className="text-emerald-500" />
          </AuiIf>
          <AuiIf condition={(s) => !s.message.isCopied}>
            <CopyIcon />
          </AuiIf>
        </TooltipIconButton>
      </ActionBarPrimitive.Copy>
      <ActionBarPrimitive.Reload asChild>
        <TooltipIconButton tooltip="重新生成">
          <RefreshCwIcon />
        </TooltipIconButton>
      </ActionBarPrimitive.Reload>
      <ActionBarMorePrimitive.Root>
        <ActionBarMorePrimitive.Trigger asChild>
          <TooltipIconButton
            tooltip="更多"
            className="data-[state=open]:bg-accent"
          >
            <MoreHorizontalIcon />
          </TooltipIconButton>
        </ActionBarMorePrimitive.Trigger>
        <ActionBarMorePrimitive.Content
          side="bottom"
          align="start"
          className="aui-action-bar-more-content z-50 min-w-40 overflow-hidden rounded-2xl border border-muted/60 bg-background/95 backdrop-blur-xl p-1.5 text-popover-foreground shadow-2xl"
        >
          <ActionBarPrimitive.ExportMarkdown asChild>
            <ActionBarMorePrimitive.Item className="aui-action-bar-more-item flex cursor-pointer select-none items-center gap-2.5 rounded-xl px-3 py-2 text-xs font-medium outline-none hover:bg-muted transition-colors">
              <DownloadIcon className="size-3.5 opacity-70" />
              导出为 Markdown
            </ActionBarMorePrimitive.Item>
          </ActionBarPrimitive.ExportMarkdown>
        </ActionBarMorePrimitive.Content>
      </ActionBarMorePrimitive.Root>
    </ActionBarPrimitive.Root>
  );
};

const AssistantMessage: FC = () => {
  return (
    <MessagePrimitive.Root
      className="aui-assistant-message-root fade-in slide-in-from-bottom-2 relative mx-auto w-full max-w-(--thread-max-width) animate-in py-6 px-2 duration-300 ease-out"
      data-role="assistant"
    >
      <div className="aui-assistant-message-content wrap-break-word px-2 text-foreground leading-relaxed selection:bg-primary/10">
        <AssistantTurnArtifacts />
        <MessagePrimitive.Content components={{ Text: MarkdownText }} />
        <MessageError />
      </div>

      <div className="aui-assistant-message-footer mt-4 ml-2 flex min-h-6 items-center opacity-0 animate-in fade-in fill-mode-forwards delay-500 duration-500">
        <BranchPicker />
        <div className="h-4 w-px bg-muted/40 mx-2" />
        <AssistantActionBar />
      </div>
    </MessagePrimitive.Root>
  );
};

const UserActionBar: FC = () => {
  return (
    <ActionBarPrimitive.Root
      hideWhenRunning
      autohide="not-last"
      className="aui-user-action-bar-root flex flex-col items-end"
    >
      <ActionBarPrimitive.Edit asChild>
        <TooltipIconButton
          tooltip="修改消息"
          className="aui-user-action-edit size-8 rounded-full hover:bg-muted"
        >
          <PencilIcon className="size-3.5" />
        </TooltipIconButton>
      </ActionBarPrimitive.Edit>
    </ActionBarPrimitive.Root>
  );
};

const UserMessage: FC = () => {
  return (
    <MessagePrimitive.Root
      className="aui-user-message-root fade-in slide-in-from-bottom-2 mx-auto grid w-full max-w-(--thread-max-width) animate-in auto-rows-auto grid-cols-[minmax(72px,1fr)_auto] content-start gap-y-2 px-2 py-6 duration-300 ease-out [&:where(>*)]:col-start-2"
      data-role="user"
    >
      <UserMessageAttachments />

      <div className="aui-user-message-content-wrapper relative col-start-2 min-w-0">
        <div className="aui-user-message-content wrap-break-word rounded-[24px] bg-muted/50 px-5 py-3 text-foreground border border-muted/40 shadow-sm">
          <MessagePrimitive.Parts />
        </div>
        <div className="aui-user-action-bar-wrapper absolute top-1/2 left-0 -translate-x-full -translate-y-1/2 pr-3 opacity-0 group-hover:opacity-100 transition-opacity">
          <UserActionBar />
        </div>
      </div>

      <BranchPicker className="aui-user-branch-picker col-span-full col-start-1 row-start-3 -mr-1 mt-2 justify-end" />
    </MessagePrimitive.Root>
  );
};

const EditComposer: FC = () => {
  return (
    <MessagePrimitive.Root className="aui-edit-composer-wrapper mx-auto flex w-full max-w-(--thread-max-width) flex-col px-2 py-6 animate-in fade-in duration-300">
      <ComposerPrimitive.Root className="aui-edit-composer-root ml-auto flex w-full max-w-[90%] flex-col rounded-[24px] bg-muted/50 border border-primary/20 shadow-xl">
        <ComposerPrimitive.Input
          className="aui-edit-composer-input min-h-24 w-full resize-none bg-transparent p-5 text-foreground text-sm leading-relaxed outline-none"
          autoFocus
        />
        <div className="aui-edit-composer-footer mx-4 mb-4 flex items-center gap-2 self-end">
          <ComposerPrimitive.Cancel asChild>
            <Button
              variant="ghost"
              size="sm"
              className="rounded-xl text-xs font-bold uppercase tracking-wider"
            >
              取消
            </Button>
          </ComposerPrimitive.Cancel>
          <ComposerPrimitive.Send asChild>
            <Button
              size="sm"
              className="rounded-xl px-5 text-xs font-bold uppercase tracking-wider shadow-lg shadow-primary/10"
            >
              更新消息
            </Button>
          </ComposerPrimitive.Send>
        </div>
      </ComposerPrimitive.Root>
    </MessagePrimitive.Root>
  );
};

const BranchPicker: FC<BranchPickerPrimitive.Root.Props> = ({
  className,
  ...rest
}) => {
  return (
    <BranchPickerPrimitive.Root
      hideWhenSingleBranch
      className={cn(
        "aui-branch-picker-root inline-flex items-center text-muted-foreground text-[10px] font-bold uppercase tracking-widest",
        className,
      )}
      {...rest}
    >
      <BranchPickerPrimitive.Previous asChild>
        <TooltipIconButton tooltip="上一条">
          <ChevronLeftIcon className="size-3" />
        </TooltipIconButton>
      </BranchPickerPrimitive.Previous>
      <span className="aui-branch-picker-state mx-1 opacity-60">
        <BranchPickerPrimitive.Number /> / <BranchPickerPrimitive.Count />
      </span>
      <BranchPickerPrimitive.Next asChild>
        <TooltipIconButton tooltip="下一条">
          <ChevronRightIcon className="size-3" />
        </TooltipIconButton>
      </BranchPickerPrimitive.Next>
    </BranchPickerPrimitive.Root>
  );
};

export const Thread: FC = () => {
  const session = useActiveChatSession();

  return (
    <ThreadPrimitive.Root
      className="aui-root aui-thread-root @container relative flex h-full min-h-0 w-full flex-1 flex-col bg-background overflow-hidden"
      style={{
        ["--thread-max-width" as string]: "72rem",
        ["--composer-max-width" as string]: "60rem",
      }}
    >
      <ThreadPrimitive.Viewport
        autoScroll
        className="aui-thread-viewport relative flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto scroll-smooth px-4 pt-4 pb-8 custom-scrollbar"
      >
        <AuiIf condition={(s) => s.thread.isEmpty}>
          <ThreadWelcome />
        </AuiIf>

        {session && <CompactionGroups />}

        <ThreadPrimitive.Messages
          components={{
            UserMessage,
            EditComposer,
            AssistantMessage,
          }}
        />

        <div className="pointer-events-none sticky bottom-4 z-40 mx-auto mt-4 flex w-fit">
          <ThreadPrimitive.ScrollToBottom asChild>
            <button className="pointer-events-auto flex size-8 items-center justify-center rounded-full border border-border/60 bg-background/80 text-muted-foreground shadow-md backdrop-blur-sm transition-all hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-0 translate-y-0 hover:-translate-y-0.5 active:translate-y-0">
              <ArrowDownIcon className="size-4" />
            </button>
          </ThreadPrimitive.ScrollToBottom>
        </div>
      </ThreadPrimitive.Viewport>

      {/* Floating bottom composer - Truly detached from scroll */}
      <div className="z-50 pointer-events-none flex justify-center pb-8 pt-4">
        <div className="w-full max-w-(--composer-max-width) px-4 pointer-events-auto flex flex-col gap-3">
          <JobStatusArtifacts />
          <PendingAssistantArtifacts />
          <ChatStatusBanner />
          <Composer />
        </div>
      </div>
      <SubagentJobDetailsDrawer />
    </ThreadPrimitive.Root>
  );
};
