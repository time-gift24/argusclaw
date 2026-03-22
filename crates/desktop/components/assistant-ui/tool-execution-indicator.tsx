"use client";

import { makeAssistantToolUI } from "@assistant-ui/react";
import { CheckIcon, LoaderIcon, XCircleIcon } from "lucide-react";

interface ToolResult {
  value: unknown;
  durationSec: number | null;
}

export const ToolExecutionIndicator = makeAssistantToolUI({
  toolName: "*",
  render: ({ toolName, status, result }) => {
    const toolResult = result as ToolResult | undefined;
    const duration = toolResult?.durationSec;

    if (status.type === "running") {
      return (
        <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
          <LoaderIcon className="size-3 animate-spin" />
          <span>{toolName}</span>
        </span>
      );
    }

    if (status.type === "complete") {
      return (
        <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
          <CheckIcon className="size-3 text-green-500" />
          <span>{toolName}</span>
          {duration != null && (
            <span className="text-muted-foreground/60">{duration.toFixed(1)}s</span>
          )}
        </span>
      );
    }

    return (
      <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
        <XCircleIcon className="size-3 text-red-500" />
        <span>{toolName}</span>
      </span>
    );
  },
});
