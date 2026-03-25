"use client";

import * as React from "react";
import {
  Calendar,
  Check,
  ChevronRight,
  MessageSquare,
  Plus,
  Trash2,
} from "lucide-react";

import { useChatStore } from "@/lib/chat-store";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

function formatRelativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return "刚刚";
  if (diffMins < 60) return `${diffMins} 分钟前`;
  if (diffHours < 24) return `${diffHours} 小时前`;
  if (diffDays < 7) return `${diffDays} 天前`;
  return date.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

export function SessionSelector() {
  const activeSession = useChatStore((s) =>
    s.activeSessionKey ? s.sessionsByKey[s.activeSessionKey] : null,
  );
  const sessionList = useChatStore((s) => s.sessionList);
  const sessionListLoading = useChatStore((s) => s.sessionListLoading);
  const selectedTemplateId = useChatStore((s) => s.selectedTemplateId);
  const loadSessionList = useChatStore((s) => s.loadSessionList);
  const switchToSession = useChatStore((s) => s.switchToSession);
  const deleteSession = useChatStore((s) => s.deleteSession);
  const activateSession = useChatStore((s) => s.activateSession);
  const [open, setOpen] = React.useState(false);
  const [deleteConfirm, setDeleteConfirm] = React.useState<string | null>(null);

  // Load session list when dialog opens
  React.useEffect(() => {
    if (open) {
      void loadSessionList();
    }
  }, [open, loadSessionList]);

  const handleSwitch = (sessionId: string) => {
    void switchToSession(sessionId);
    setOpen(false);
  };

  const handleNewSession = () => {
    if (selectedTemplateId) {
      void activateSession(selectedTemplateId);
    }
    setOpen(false);
  };

  const handleDelete = (sessionId: string) => {
    void deleteSession(sessionId);
    setDeleteConfirm(null);
  };

  // Find current session in the list
  const currentSession = activeSession
    ? sessionList.find((s) => s.id === activeSession.sessionId)
    : null;

  const trigger = (
    <button
      type="button"
      className="flex h-8 items-center gap-2 px-3 rounded-full bg-muted/50 hover:bg-muted transition-all border border-transparent hover:border-muted-foreground/20 group outline-none focus-visible:ring-2 focus-visible:ring-primary/20"
    >
      <div className="flex h-4 w-4 items-center justify-center rounded-full bg-primary/10 text-primary group-hover:bg-primary group-hover:text-primary-foreground transition-colors">
        <MessageSquare className="size-3" />
      </div>
      <span className="max-w-[120px] truncate text-xs font-bold tracking-tight">
        {currentSession?.name ?? activeSession?.sessionKey ?? "新会话"}
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
              <MessageSquare className="h-5 w-5" />
            </div>
            <div className="space-y-0.5">
              <DialogTitle className="text-lg font-bold tracking-tight">
                历史会话
              </DialogTitle>
              <DialogDescription className="text-xs font-medium text-muted-foreground uppercase tracking-widest opacity-70">
                Session History
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        {/* New Session Button */}
        <div className="px-4 pb-2">
          <Button
            variant="outline"
            className="w-full justify-start gap-3 rounded-xl border-dashed border-muted-foreground/30 bg-muted/20 hover:bg-muted/40 transition-colors h-auto py-3"
            onClick={handleNewSession}
          >
            <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
              <Plus className="size-3.5" />
            </div>
            <div className="text-left">
              <div className="text-xs font-bold">新建会话</div>
              <div className="text-[10px] text-muted-foreground">
                基于当前智能体创建
              </div>
            </div>
          </Button>
        </div>

        {/* Session List */}
        <div className="px-4 pb-8 overflow-y-auto max-h-[50vh] custom-scrollbar">
          {sessionListLoading ? (
            <div className="flex items-center justify-center py-8">
              <div className="h-5 w-5 border-2 border-primary border-t-transparent rounded-full animate-spin" />
            </div>
          ) : sessionList.length === 0 ? (
            <div className="text-center py-8 text-muted-foreground text-sm">
              暂无历史会话
            </div>
          ) : (
            <div className="space-y-2">
              {sessionList.map((session) => {
                const isActive = activeSession?.sessionId === session.id;
                return (
                  <div
                    key={session.id}
                    className={cn(
                      "group flex items-center gap-3 rounded-xl border p-3 transition-all",
                      isActive
                        ? "border-primary bg-primary/5 shadow-sm"
                        : "border-muted/60 hover:border-primary/30 hover:bg-muted/20",
                    )}
                  >
                    <div
                      className={cn(
                        "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg transition-colors",
                        isActive
                          ? "bg-primary text-primary-foreground"
                          : "bg-muted text-muted-foreground group-hover:bg-primary/10 group-hover:text-primary",
                      )}
                    >
                      <MessageSquare className="size-3.5" />
                    </div>

                    <button
                      className="flex-1 text-left min-w-0"
                      onClick={() => handleSwitch(session.id)}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-sm font-bold tracking-tight truncate">
                          {session.name}
                        </span>
                        {isActive && <Check className="size-3.5 text-primary shrink-0" />}
                      </div>
                      <div className="flex items-center gap-3 mt-0.5">
                        <div className="flex items-center gap-1 text-[10px] text-muted-foreground">
                          <Calendar className="size-2.5" />
                          {formatRelativeTime(session.updated_at)}
                        </div>
                        <div className="text-[10px] text-muted-foreground">
                          {session.thread_count} 个对话
                        </div>
                      </div>
                    </button>

                    {/* Delete button */}
                    {deleteConfirm === session.id ? (
                      <div className="flex items-center gap-1 shrink-0">
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-2 text-xs text-destructive hover:bg-destructive/10"
                          onClick={() => handleDelete(session.id)}
                        >
                          删除
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-2 text-xs"
                          onClick={() => setDeleteConfirm(null)}
                        >
                          取消
                        </Button>
                      </div>
                    ) : (
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-8 w-8 p-0 opacity-0 group-hover:opacity-100 shrink-0 text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-all"
                        onClick={(e) => {
                          e.stopPropagation();
                          setDeleteConfirm(session.id);
                        }}
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        <div className="bg-muted/10 px-8 py-4 border-t border-muted/60 flex items-center justify-center">
          <p className="text-[10px] text-muted-foreground font-medium uppercase tracking-tighter opacity-50">
            {sessionList.length} 个会话 · 选择一个继续对话
          </p>
        </div>
      </DialogContent>
    </Dialog>
  );
}
