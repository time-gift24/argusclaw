"use client";

import { useChatStore } from "@/lib/chat-store";

export function AgentSelector() {
  const templates = useChatStore((state) => state.templates);
  const selectedTemplateId = useChatStore((state) => state.selectedTemplateId);
  const activateSession = useChatStore((state) => state.activateSession);

  if (templates.length === 0) return null;

  return (
    <select
      value={selectedTemplateId ?? ""}
      onChange={(e) => void activateSession(e.target.value)}
      className="rounded-md border border-input bg-background px-2 py-1 text-sm"
    >
      {templates.map((template) => (
        <option key={template.id} value={template.id}>
          {template.display_name}
        </option>
      ))}
    </select>
  );
}
