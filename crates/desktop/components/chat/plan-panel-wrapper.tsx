"use client";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { PlanPanel } from "@/components/chat/plan-panel";

export function PlanPanelWrapper() {
  const session = useActiveChatSession();
  const plan = session?.pendingAssistant?.plan;

  if (!plan || plan.length === 0) {
    return null;
  }

  return <PlanPanel plan={plan} />;
}
