"use client";

import { memo, useCallback, useRef, useState } from "react";
import {
  AlertCircleIcon,
  CheckIcon,
  ChevronDownIcon,
  Loader2,
  XCircleIcon,
} from "lucide-react";
import {
  useScrollLock,
  type ToolCallMessagePartStatus,
  type ToolCallMessagePartComponent,
} from "@assistant-ui/react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";

const ANIMATION_DURATION = 200;

export type ToolFallbackRootProps = Omit<
  React.ComponentProps<typeof Collapsible>,
  "open" | "onOpenChange"
> & {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  defaultOpen?: boolean;
};

function ToolFallbackRoot({
  className,
  open: controlledOpen,
  onOpenChange: controlledOnOpenChange,
  defaultOpen = false,
  children,
  ...props
}: ToolFallbackRootProps) {
  const collapsibleRef = useRef<HTMLDivElement>(null);
  const [uncontrolledOpen, setUncontrolledOpen] = useState(defaultOpen);
  const lockScroll = useScrollLock(collapsibleRef, ANIMATION_DURATION);

  const isControlled = controlledOpen !== undefined;
  const isOpen = isControlled ? controlledOpen : uncontrolledOpen;

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        lockScroll();
      }
      if (!isControlled) {
        setUncontrolledOpen(open);
      }
      controlledOnOpenChange?.(open);
    },
    [lockScroll, isControlled, controlledOnOpenChange],
  );

  return (
    <Collapsible
      ref={collapsibleRef}
      data-slot="tool-fallback-root"
      open={isOpen}
      onOpenChange={handleOpenChange}
      className={cn(
        "aui-tool-fallback-root group/tool-fallback-root w-full py-0.5",
        className,
      )}
      style={
        {
          "--animation-duration": `${ANIMATION_DURATION}ms`,
        } as React.CSSProperties
      }
      {...props}
    >
      {children}
    </Collapsible>
  );
}

function ToolFallbackTrigger({
  toolName,
  status,
  className,
  ...props
}: React.ComponentProps<typeof CollapsibleTrigger> & {
  toolName: string;
  status?: ToolCallMessagePartStatus;
}) {
  const statusType = status?.type ?? "complete";
  const isRunning = statusType === "running";
  const isCancelled =
    status?.type === "incomplete" && status.reason === "cancelled";
  const isFailed = status?.type === "incomplete" && status.reason !== "cancelled";

  return (
    <CollapsibleTrigger
      data-slot="tool-fallback-trigger"
      className={cn(
        "aui-tool-fallback-trigger group/trigger flex w-full items-center gap-2 rounded-lg border border-transparent px-2 py-1 text-xs text-muted-foreground transition-all hover:border-muted/40 hover:bg-muted/50",
        className,
      )}
      {...props}
    >
      <div className={cn(
        "flex h-5 w-5 shrink-0 items-center justify-center rounded-md",
        isRunning ? "bg-primary/10 text-primary" :
        isFailed ? "bg-destructive/10 text-destructive" :
        isCancelled ? "bg-muted text-muted-foreground" : "bg-emerald-500/10 text-emerald-600"
      )}>
        {isRunning ? <Loader2 className="size-3 animate-spin" /> :
         isFailed ? <AlertCircleIcon className="size-3" /> :
         isCancelled ? <XCircleIcon className="size-3" /> : <CheckIcon className="size-3" />}
      </div>

      <span
        data-slot="tool-fallback-trigger-label"
        className={cn(
          "aui-tool-fallback-trigger-label-wrapper relative inline-block grow text-left leading-none font-medium",
          isCancelled && "text-muted-foreground line-through opacity-60",
        )}
      >
        <span className="opacity-80">
          {isCancelled ? "已取消" : isRunning ? "正在调用" : isFailed ? "调用失败" : "已调用"}: <code className="bg-muted px-1.5 py-0.5 rounded font-mono text-[10px] ml-1">{toolName}</code>
        </span>
      </span>

      <ChevronDownIcon
        data-slot="tool-fallback-trigger-chevron"
        className={cn(
          "aui-tool-fallback-trigger-chevron size-3 shrink-0 opacity-30",
          "transition-transform duration-(--animation-duration) ease-out",
          "group-not-data-[panel-open]/trigger:-rotate-90",
          "group-data-[panel-open]/trigger:rotate-0",
        )}
      />
    </CollapsibleTrigger>
  );
}

function ToolFallbackContent({
  className,
  children,
  ...props
}: React.ComponentProps<typeof CollapsibleContent>) {
  return (
    <CollapsibleContent
      data-slot="tool-fallback-content"
      className={cn(
        "aui-tool-fallback-content relative overflow-hidden text-xs outline-none",
        "group/collapsible-content ease-out",
        "data-[closed]:animate-collapsible-up",
        "data-[open]:animate-collapsible-down",
        "data-[closed]:fill-mode-forwards",
        "data-[closed]:pointer-events-none",
        "data-[open]:duration-(--animation-duration)",
        "data-[closed]:duration-(--animation-duration)",
        className,
      )}
      {...props}
    >
      <div className="mt-1 ml-4.5 flex flex-col gap-1.5 border-l-2 border-muted/40 py-1.5 pl-3">{children}</div>
    </CollapsibleContent>
  );
}

function ToolFallbackArgs({
  argsText,
  className,
  ...props
}: React.ComponentProps<"div"> & {
  argsText?: string;
}) {
  if (!argsText) return null;

  return (
    <div
      data-slot="tool-fallback-args"
      className={cn("aui-tool-fallback-args space-y-1.5", className)}
      {...props}
    >
      <p className="text-[10px] font-bold uppercase tracking-widest opacity-50 ml-1">工具参数</p>
      <pre className="aui-tool-fallback-args-value max-h-[16rem] overflow-auto custom-scrollbar whitespace-pre-wrap rounded-lg border border-muted/40 bg-muted/20 p-2.5 font-mono text-[11px] text-muted-foreground">
        {argsText}
      </pre>
    </div>
  );
}

function ToolFallbackResult({
  result,
  className,
  ...props
}: React.ComponentProps<"div"> & {
  result?: unknown;
}) {
  if (result === undefined) return null;

  return (
    <div
      data-slot="tool-fallback-result"
      className={cn(
        "aui-tool-fallback-result space-y-1.5",
        className,
      )}
      {...props}
    >
      <p className="text-[10px] font-bold uppercase tracking-widest opacity-50 ml-1">工具输出</p>
      <pre className="aui-tool-fallback-result-content max-h-[16rem] overflow-auto custom-scrollbar whitespace-pre-wrap rounded-lg border border-emerald-500/10 bg-emerald-500/5 p-2.5 font-mono text-[11px] text-muted-foreground">
        {typeof result === "string" ? result : JSON.stringify(result, null, 2)}
      </pre>
    </div>
  );
}

function ToolFallbackError({
  status,
  className,
  ...props
}: React.ComponentProps<"div"> & {
  status?: ToolCallMessagePartStatus;
}) {
  if (status?.type !== "incomplete") return null;

  const error = status.error;
  const errorText = error
    ? typeof error === "string"
      ? error
      : JSON.stringify(error)
    : null;

  if (!errorText) return null;

  const isCancelled = status.reason === "cancelled";

  return (
    <div
      data-slot="tool-fallback-error"
      className={cn("aui-tool-fallback-error space-y-1.5", className)}
      {...props}
    >
      <p className="text-[10px] font-bold uppercase tracking-widest text-destructive/70 ml-1">
        {isCancelled ? "已取消" : "错误详情"}
      </p>
      <p className="text-[11px] text-destructive bg-destructive/5 p-3 rounded-xl border border-destructive/10 leading-relaxed font-medium">
        {errorText}
      </p>
    </div>
  );
}

export const ToolFallbackImpl: ToolCallMessagePartComponent = ({
  toolName,
  argsText,
  result,
  status,
}) => {
  const isCancelled =
    status?.type === "incomplete" && status.reason === "cancelled";

  return (
    <ToolFallbackRoot>
      <ToolFallbackTrigger toolName={toolName} status={status} />
      <ToolFallbackContent>
        <ToolFallbackError status={status} />
        <ToolFallbackArgs
          argsText={argsText}
          className={cn(isCancelled && "opacity-60")}
        />
        {!isCancelled && <ToolFallbackResult result={result} />}
      </ToolFallbackContent>
    </ToolFallbackRoot>
  );
};

const ToolFallback = memo(
  ToolFallbackImpl,
) as unknown as ToolCallMessagePartComponent & {
  Root: typeof ToolFallbackRoot;
  Trigger: typeof ToolFallbackTrigger;
  Content: typeof ToolFallbackContent;
  Args: typeof ToolFallbackArgs;
  Result: typeof ToolFallbackResult;
  Error: typeof ToolFallbackError;
};

ToolFallback.displayName = "ToolFallback";
ToolFallback.Root = ToolFallbackRoot;
ToolFallback.Trigger = ToolFallbackTrigger;
ToolFallback.Content = ToolFallbackContent;
ToolFallback.Args = ToolFallbackArgs;
ToolFallback.Result = ToolFallbackResult;
ToolFallback.Error = ToolFallbackError;

export {
  ToolFallback,
  ToolFallbackRoot,
  ToolFallbackTrigger,
  ToolFallbackContent,
  ToolFallbackArgs,
  ToolFallbackResult,
  ToolFallbackError,
};
