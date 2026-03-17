"use client";

import { useChatStore } from "@/lib/chat-store";
import { cn } from "@/lib/utils";

// Default context window size (most modern models support at least 128K)
const DEFAULT_CONTEXT_WINDOW = 128000;

export function ContextStatus() {
  const activeSessionKey = useChatStore((state) => state.activeSessionKey);
  const sessionsByKey = useChatStore((state) => state.sessionsByKey);
  const providers = useChatStore((state) => state.providers);

  const session = activeSessionKey ? sessionsByKey[activeSessionKey] : null;
  const tokenCount = session?.tokenCount ?? 0;

  // Get effective provider
  const provider = session?.effectiveProviderId
    ? providers.find((p) => p.id === session.effectiveProviderId)
    : providers.find((p) => p.is_default);

  const model = session?.effectiveModel ?? provider?.default_model ?? "";

  // Get context window from model config if available, otherwise use default
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const contextWindow = (provider as any)?.model_config?.[model]?.context_length ?? DEFAULT_CONTEXT_WINDOW;

  // Calculate usage percentage
  const usagePercent = contextWindow > 0 ? (tokenCount / contextWindow) * 100 : 0;

  // Format token count (e.g., "12.5K", "1.2M")
  const formatTokens = (n: number): string => {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
  };

  // Color coding based on usage
  // < 50%: muted, 50-80%: warning, > 80%: critical
  const getColorClass = () => {
    if (usagePercent >= 80) return "text-destructive";
    if (usagePercent >= 50) return "text-amber-500";
    return "text-muted-foreground";
  };

  // Don't show if no tokens used
  if (tokenCount === 0) return null;

  return (
    <span
      className={cn(
        "text-[10px] font-medium tabular-nums",
        getColorClass()
      )}
      title={`${formatTokens(tokenCount)} / ${formatTokens(contextWindow)} tokens (${usagePercent.toFixed(0)}%)`}
    >
      {formatTokens(tokenCount)}
    </span>
  );
}
