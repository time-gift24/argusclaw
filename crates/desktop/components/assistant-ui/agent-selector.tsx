"use client";

import * as React from "react";
import { Bot, Check, ChevronRight, Layers, Sparkles } from "lucide-react";

import { useChatStore } from "@/lib/chat-store";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

export function AgentSelector() {
  const templates = useChatStore((state) => state.templates);
  const activeSession = useChatStore((state) =>
    state.activeSessionKey ? state.sessionsByKey[state.activeSessionKey] ?? null : null,
  );
  const selectedTemplateId = useChatStore((state) => state.selectedTemplateId);
  const selectTemplate = useChatStore((state) => state.selectTemplate);
  const activateSession = useChatStore((state) => state.activateSession);
  const [open, setOpen] = React.useState(false);
  const [confirmOpen, setConfirmOpen] = React.useState(false);
  const [pendingTemplateId, setPendingTemplateId] = React.useState<number | null>(null);

  const currentTemplateId = activeSession?.templateId ?? selectedTemplateId;
  const selectedTemplate = templates.find((t) => t.id === currentTemplateId);
  const pendingTemplate = templates.find((t) => t.id === pendingTemplateId);

  const handleCancelPendingSwitch = React.useCallback(() => {
    if (activeSession) {
      void selectTemplate(activeSession.templateId);
    }
    setPendingTemplateId(null);
    setConfirmOpen(false);
  }, [activeSession, selectTemplate]);

  const handleConfirmPendingSwitch = React.useCallback(() => {
    if (pendingTemplateId == null) return;
    setConfirmOpen(false);
    void activateSession(pendingTemplateId);
    setPendingTemplateId(null);
  }, [activateSession, pendingTemplateId]);

  if (templates.length === 0) return null;

  const handleSelect = (templateId: number) => {
    if (activeSession && activeSession.templateId !== templateId) {
      setPendingTemplateId(templateId);
      setOpen(false);
      setConfirmOpen(true);
      return;
    }

    void selectTemplate(templateId);
    setOpen(false);
  };

  // Group templates into parents and their children
  const parentAgents = templates.filter(t => !t.parent_agent_id && t.agent_type !== "subagent");
  const subagents = templates.filter(t => t.parent_agent_id || t.agent_type === "subagent");

  const trigger = (
    <button
      type="button"
      className="flex h-8 items-center gap-2 px-3 rounded-full bg-muted/50 hover:bg-muted transition-all border border-transparent hover:border-muted-foreground/20 group outline-none focus-visible:ring-2 focus-visible:ring-primary/20"
    >
      <div className="flex h-4 w-4 items-center justify-center rounded-full bg-primary/10 text-primary group-hover:bg-primary group-hover:text-primary-foreground transition-colors">
        <Bot className="size-3" />
      </div>
      <span className="max-w-[120px] truncate text-xs font-bold tracking-tight">
        {selectedTemplate?.display_name ?? "选择智能体"}
      </span>
      <ChevronRight className="size-3 opacity-40 group-hover:opacity-100 group-hover:translate-x-0.5 transition-all" />
    </button>
  );

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={trigger} />
      <DialogContent className="sm:max-w-[500px] p-0 overflow-hidden border-none shadow-2xl rounded-[28px] bg-background">
        <DialogHeader className="px-8 pt-8 pb-4">
          <div className="flex items-center gap-3">
            <div className="bg-primary/10 p-2 rounded-xl text-primary">
              <Sparkles className="h-5 w-5" />
            </div>
            <div className="space-y-0.5">
              <DialogTitle className="text-lg font-bold tracking-tight">选择对话智能体</DialogTitle>
              <DialogDescription className="text-xs font-medium text-muted-foreground uppercase tracking-widest opacity-70">
                Switch Agent Template
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="px-4 pb-8 overflow-y-auto max-h-[60vh] custom-scrollbar">
          <div className="grid gap-6">
            {parentAgents.map((parent) => (
              <div key={parent.id} className="space-y-3">
                {/* Parent Agent Item */}
                <button
                  onClick={() => handleSelect(parent.id)}
                  className={cn(
                    "w-full group flex items-center gap-4 rounded-2xl border p-4 text-left transition-all",
                    currentTemplateId === parent.id 
                      ? "border-primary bg-primary/5 shadow-sm" 
                      : "border-muted/60 hover:border-primary/30 hover:bg-muted/30"
                  )}
                >
                  <div className={cn(
                    "flex h-10 w-10 shrink-0 items-center justify-center rounded-xl transition-colors",
                    currentTemplateId === parent.id ? "bg-primary text-primary-foreground" : "bg-muted text-primary group-hover:bg-primary group-hover:text-primary-foreground"
                  )}>
                    <Bot className="size-5" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-bold text-sm tracking-tight">{parent.display_name}</span>
                      <div className="flex items-center gap-2">
                        <Badge variant="outline" className="text-[9px] h-4 px-1 opacity-50 font-mono">v{parent.version}</Badge>
                        {currentTemplateId === parent.id && <Check className="size-4 text-primary" />}
                      </div>
                    </div>
                    {parent.description && (
                      <p className="text-[11px] text-muted-foreground mt-1 line-clamp-1 leading-relaxed">
                        {parent.description}
                      </p>
                    )}
                  </div>
                </button>

                {/* Subagents Group */}
                {subagents.some(s => s.parent_agent_id === parent.id) && (
                  <div className="grid gap-2 ml-8 pl-4 border-l-2 border-primary/10">
                    {subagents
                      .filter(s => s.parent_agent_id === parent.id)
                      .map((sub) => (
                        <button
                          key={sub.id}
                          onClick={() => handleSelect(sub.id)}
                          className={cn(
                            "w-full group flex items-center gap-3 rounded-xl border p-3 text-left transition-all relative",
                            currentTemplateId === sub.id 
                              ? "border-primary/40 bg-primary/5 shadow-inner" 
                              : "border-muted/40 hover:border-primary/20 hover:bg-muted/20"
                          )}
                        >
                          <div className="absolute -left-[18px] top-1/2 w-2 h-[2px] bg-primary/10" />
                          <div className={cn(
                            "flex h-7 w-7 shrink-0 items-center justify-center rounded-lg transition-colors",
                            currentTemplateId === sub.id ? "bg-primary/20 text-primary" : "bg-muted/50 text-muted-foreground group-hover:bg-primary/10 group-hover:text-primary"
                          )}>
                            <Layers className="size-3.5" />
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center justify-between gap-2">
                              <span className="font-semibold text-xs tracking-tight">{sub.display_name}</span>
                              {currentTemplateId === sub.id && <Check className="size-3 text-primary" />}
                            </div>
                            {sub.description && (
                              <p className="text-[10px] text-muted-foreground mt-0.5 line-clamp-1 opacity-70">
                                {sub.description}
                              </p>
                            )}
                          </div>
                        </button>
                      ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
        
        <div className="bg-muted/10 px-8 py-4 border-t border-muted/60 flex items-center justify-center">
          <p className="text-[10px] text-muted-foreground font-medium uppercase tracking-tighter opacity-50">
            Select an agent to begin specialized task processing
          </p>
        </div>
      </DialogContent>
      <Dialog open={confirmOpen} onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          handleCancelPendingSwitch();
          return;
        }
        setConfirmOpen(true);
      }}>
        <DialogContent
          showCloseButton={false}
          className="sm:max-w-md rounded-[24px] border-none bg-background p-6 shadow-2xl"
        >
          <DialogHeader className="space-y-2">
            <DialogTitle className="text-base font-bold tracking-tight">
              切换智能体需要新建会话
            </DialogTitle>
            <DialogDescription>
              已选择 {pendingTemplate?.display_name ?? "新的智能体"}。当前会话仍在使用{" "}
              {selectedTemplate?.display_name ?? "当前智能体"}，需要新建会话才能生效。是否立即新建？
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button variant="ghost" onClick={handleCancelPendingSwitch}>
              继续当前会话
            </Button>
            <Button onClick={handleConfirmPendingSwitch}>
              新建会话
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Dialog>
  );
}
