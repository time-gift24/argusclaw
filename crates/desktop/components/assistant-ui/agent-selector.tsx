"use client";

import * as React from "react";
import { BotIcon, CheckIcon } from "lucide-react";

import { useChatStore } from "@/lib/chat-store";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

export function AgentSelector() {
  const templates = useChatStore((state) => state.templates);
  const selectedTemplateId = useChatStore((state) => state.selectedTemplateId);
  const activeSessionId = useChatStore((state) => state.activeSessionId);
  const selectedProviderPreferenceId = useChatStore((state) => state.selectedProviderPreferenceId);
  const activateSession = useChatStore((state) => state.activateSession);
  const [open, setOpen] = React.useState(false);

  if (templates.length === 0) return null;

  const selectedTemplate = templates.find((t) => t.id === selectedTemplateId);

  const handleSelect = (templateId: number) => {
    void activateSession(activeSessionId ?? 0, templateId, selectedProviderPreferenceId);
    setOpen(false);
  };

  const trigger = (
    <Button
      variant="ghost"
      size="sm"
      className="h-7 gap-1.5 px-2 text-xs text-muted-foreground hover:text-foreground"
    >
      <BotIcon className="size-3.5" />
      <span className="max-w-[80px] truncate">
        {selectedTemplate?.display_name ?? "选择 Agent"}
      </span>
    </Button>
  );

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={trigger} />
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>选择 Agent</DialogTitle>
        </DialogHeader>
        <div className="grid gap-2 py-2">
          {templates.map((template) => (
            <button
              key={template.id}
              onClick={() => handleSelect(template.id)}
              className={cn(
                "flex items-start gap-3 rounded-lg border p-3 text-left transition-colors hover:bg-accent",
                selectedTemplateId === template.id && "border-primary bg-accent/50"
              )}
            >
              <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary/10">
                <BotIcon className="size-4 text-primary" />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-sm">{template.display_name}</span>
                  {selectedTemplateId === template.id && (
                    <CheckIcon className="size-4 text-primary" />
                  )}
                </div>
                {template.description && (
                  <p className="text-xs text-muted-foreground mt-0.5 line-clamp-2">
                    {template.description}
                  </p>
                )}
              </div>
            </button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
