"use client";

import * as React from "react";
import { createPortal } from "react-dom";
import {
  Calendar,
  Check,
  History,
  Loader2,
  MessageSquare,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react";

import { TooltipIconButton } from "@/components/assistant-ui/tooltip-icon-button";
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
import { Input } from "@/components/ui/input";
import { useChatStore } from "@/lib/chat-store";
import { sessions, type SessionSummary, type ThreadSummary } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type ContextMenuState =
  | {
      kind: "session";
      sessionId: string;
      currentValue: string;
      x: number;
      y: number;
    }
  | {
      kind: "thread";
      sessionId: string;
      threadId: string;
      currentValue: string;
      x: number;
      y: number;
    };

type RenameTarget =
  | {
      kind: "session";
      sessionId: string;
      currentValue: string;
    }
  | {
      kind: "thread";
      sessionId: string;
      threadId: string;
      currentValue: string;
    };

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

function formatStableTime(dateStr: string): string {
  return dateStr.replace("T", " ").slice(0, 16);
}

function displaySessionName(session: SessionSummary): string {
  return session.name.trim() ? session.name : session.id;
}

function displayThreadName(thread: ThreadSummary): string {
  return thread.title && thread.title.trim() ? thread.title : thread.thread_id;
}

function getContextMenuPosition(x: number, y: number) {
  if (typeof window === "undefined") {
    return { x, y };
  }

  const menuWidth = 180;
  const menuHeight = 120;
  const padding = 12;

  return {
    x: Math.min(x, window.innerWidth - menuWidth - padding),
    y: Math.min(y, window.innerHeight - menuHeight - padding),
  };
}

export function NewSessionButton() {
  const selectedTemplateId = useChatStore((s) => s.selectedTemplateId);
  const activateSession = useChatStore((s) => s.activateSession);

  const handleNewSession = () => {
    if (!selectedTemplateId) return;
    void activateSession(selectedTemplateId);
  };

  return (
    <TooltipIconButton
      tooltip="新建会话"
      side="top"
      type="button"
      className="size-8 rounded-full"
      aria-label="新建会话"
      onClick={handleNewSession}
      disabled={!selectedTemplateId}
    >
      <Plus className="size-4" />
    </TooltipIconButton>
  );
}

export function SessionHistoryButton() {
  const activeSession = useChatStore((s) =>
    s.activeSessionKey ? s.sessionsByKey[s.activeSessionKey] : null,
  );
  const sessionList = useChatStore((s) => s.sessionList);
  const sessionListLoading = useChatStore((s) => s.sessionListLoading);
  const threadListBySessionId = useChatStore((s) => s.threadListBySessionId);
  const threadListLoadingBySessionId = useChatStore((s) => s.threadListLoadingBySessionId);
  const loadSessionList = useChatStore((s) => s.loadSessionList);
  const loadThreads = useChatStore((s) => s.loadThreads);
  const switchToThread = useChatStore((s) => s.switchToThread);
  const deleteSession = useChatStore((s) => s.deleteSession);
  const [open, setOpen] = React.useState(false);
  const [selectedSessionId, setSelectedSessionId] = React.useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = React.useState<string | null>(null);
  const [contextMenu, setContextMenu] = React.useState<ContextMenuState | null>(null);
  const [renameTarget, setRenameTarget] = React.useState<RenameTarget | null>(null);
  const [renameValue, setRenameValue] = React.useState("");
  const [renameSaving, setRenameSaving] = React.useState(false);
  const [hasMounted, setHasMounted] = React.useState(false);
  const contextMenuRef = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    setHasMounted(true);
  }, []);

  React.useEffect(() => {
    if (open) {
      void loadSessionList();
    } else {
      setContextMenu(null);
      setDeleteConfirm(null);
    }
  }, [open, loadSessionList]);

  React.useEffect(() => {
    if (!open) return;
    const preferredSessionId = activeSession?.sessionId ?? sessionList[0]?.id ?? null;
    setSelectedSessionId((current) => {
      if (current && sessionList.some((session) => session.id === current)) {
        return current;
      }
      return preferredSessionId;
    });
  }, [open, activeSession?.sessionId, sessionList]);

  React.useEffect(() => {
    if (open && selectedSessionId) {
      void loadThreads(selectedSessionId);
    }
  }, [open, selectedSessionId, loadThreads]);

  React.useEffect(() => {
    if (!contextMenu) return;

    const handleWindowPointerDown = (event: PointerEvent) => {
      if (contextMenuRef.current?.contains(event.target as Node)) {
        return;
      }
      setContextMenu(null);
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setContextMenu(null);
      }
    };
    const handleViewportChange = () => setContextMenu(null);

    window.addEventListener("pointerdown", handleWindowPointerDown);
    window.addEventListener("keydown", handleEscape);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    return () => {
      window.removeEventListener("pointerdown", handleWindowPointerDown);
      window.removeEventListener("keydown", handleEscape);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
    };
  }, [contextMenu]);

  const renderTimestamp = (dateStr: string) =>
    hasMounted ? formatRelativeTime(dateStr) : formatStableTime(dateStr);

  const handleSwitchThread = (sessionId: string, threadId: string) => {
    void switchToThread(sessionId, threadId);
    setOpen(false);
  };

  const handleDelete = (sessionId: string) => {
    void deleteSession(sessionId);
    setDeleteConfirm(null);
  };

  const openRenameDialog = (target: RenameTarget) => {
    setRenameTarget(target);
    setRenameValue(target.currentValue);
    setContextMenu(null);
  };

  const handleRenameSubmit = async () => {
    if (!renameTarget) return;

    setRenameSaving(true);
    try {
      if (renameTarget.kind === "session") {
        await sessions.renameSession(renameTarget.sessionId, renameValue.trim());
        await loadSessionList();
      } else {
        await sessions.renameThread(
          renameTarget.sessionId,
          renameTarget.threadId,
          renameValue.trim(),
        );
        await loadThreads(renameTarget.sessionId);
      }
      setRenameTarget(null);
      setRenameValue("");
    } finally {
      setRenameSaving(false);
    }
  };

  const selectedSession = selectedSessionId
    ? sessionList.find((session) => session.id === selectedSessionId) ?? null
    : null;
  const sessionThreads = selectedSessionId
    ? threadListBySessionId[selectedSessionId] ?? []
    : [];
  const isThreadLoading = selectedSessionId
    ? threadListLoadingBySessionId[selectedSessionId] ?? false
    : false;

  const trigger = (
    <TooltipIconButton
      tooltip="历史会话"
      side="top"
      type="button"
      className="size-8 rounded-full"
      aria-label="历史会话"
    >
      <History className="size-4" />
    </TooltipIconButton>
  );

  return (
    <>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger render={trigger} />
        <DialogContent
          className="sm:max-w-[920px] p-0 overflow-hidden border-none shadow-2xl rounded-[28px] bg-background"
        >
          <DialogHeader className="px-8 pt-8 pb-4">
            <div className="flex items-center gap-3">
              <div className="bg-primary/10 p-2 rounded-xl text-primary">
                <History className="h-5 w-5" />
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

          <div className="grid min-h-[480px] grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)] border-t border-muted/60">
            <div className="border-r border-muted/60">
              <div className="border-b border-muted/60 px-5 py-3">
                <div className="text-[10px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
                  Sessions
                </div>
              </div>
              <div className="max-h-[420px] overflow-y-auto px-3 py-3 custom-scrollbar">
                {sessionListLoading ? (
                  <div className="flex items-center justify-center py-8">
                    <Loader2 className="size-4 animate-spin text-primary" />
                  </div>
                ) : sessionList.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-muted-foreground/20 px-4 py-6 text-center text-sm text-muted-foreground">
                    暂无历史会话
                  </div>
                ) : (
                  <div className="space-y-2">
                    {sessionList.map((session) => {
                      const isSelected = session.id === selectedSessionId;
                      const isActive = activeSession?.sessionId === session.id;
                      return (
                        <button
                          key={session.id}
                          type="button"
                          className={cn(
                            "w-full rounded-xl border px-3 py-3 text-left transition-all",
                            isSelected
                              ? "border-primary bg-primary/5 shadow-sm"
                              : "border-muted/60 hover:border-primary/30 hover:bg-muted/20",
                          )}
                          onClick={() => setSelectedSessionId(session.id)}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            const position = getContextMenuPosition(
                              event.clientX,
                              event.clientY,
                            );
                            setContextMenu({
                              kind: "session",
                              sessionId: session.id,
                              currentValue: session.name,
                              x: position.x,
                              y: position.y,
                            });
                          }}
                        >
                          <div className="flex items-center gap-2">
                            <div
                              className={cn(
                                "flex size-7 items-center justify-center rounded-lg",
                                isSelected
                                  ? "bg-primary text-primary-foreground"
                                  : "bg-muted text-muted-foreground",
                              )}
                            >
                              <MessageSquare className="size-3.5" />
                            </div>
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center justify-between gap-2">
                                <span className="truncate text-sm font-semibold">
                                  {session.name.trim() ? session.name : session.id}
                                </span>
                                {isActive ? (
                                  <Check className="size-3.5 shrink-0 text-primary" />
                                ) : null}
                              </div>
                              <div className="mt-1 flex items-center gap-3 text-[10px] text-muted-foreground">
                                <span className="flex items-center gap-1">
                                  <Calendar className="size-2.5" />
                                  {renderTimestamp(session.updated_at)}
                                </span>
                                <span>{session.thread_count} 个对话</span>
                              </div>
                            </div>
                          </div>
                          {deleteConfirm === session.id ? (
                            <div className="mt-3 flex items-center gap-1">
                              <Button
                                size="sm"
                                variant="ghost"
                                className="h-7 px-2 text-xs text-destructive hover:bg-destructive/10"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  handleDelete(session.id);
                                }}
                              >
                                删除
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                className="h-7 px-2 text-xs"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  setDeleteConfirm(null);
                                }}
                              >
                                取消
                              </Button>
                            </div>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>

            <div className="flex min-h-0 flex-col">
              <div className="border-b border-muted/60 px-5 py-3">
                <div className="text-[10px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
                  Threads
                </div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {selectedSession ? displaySessionName(selectedSession) : "请选择左侧 Session"}
                </div>
              </div>

              <div className="max-h-[420px] overflow-y-auto px-3 py-3 custom-scrollbar">
                {!selectedSessionId ? (
                  <div className="rounded-xl border border-dashed border-muted-foreground/20 px-4 py-6 text-center text-sm text-muted-foreground">
                    请选择左侧 Session
                  </div>
                ) : isThreadLoading ? (
                  <div className="flex items-center justify-center py-8">
                    <Loader2 className="size-4 animate-spin text-primary" />
                  </div>
                ) : sessionThreads.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-muted-foreground/20 px-4 py-6 text-center text-sm text-muted-foreground">
                    暂无 Thread
                  </div>
                ) : (
                  <div className="space-y-2">
                    {sessionThreads.map((thread) => {
                      const isActiveThread =
                        activeSession?.sessionId === selectedSessionId &&
                        activeSession.threadId === thread.thread_id;

                      return (
                        <button
                          key={thread.thread_id}
                          type="button"
                          className={cn(
                            "w-full rounded-xl border px-3 py-3 text-left transition-all",
                            isActiveThread
                              ? "border-primary bg-primary/5 shadow-sm"
                              : "border-muted/60 hover:border-primary/30 hover:bg-muted/20",
                          )}
                          onClick={() =>
                            handleSwitchThread(selectedSessionId, thread.thread_id)
                          }
                          onContextMenu={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            const position = getContextMenuPosition(
                              event.clientX,
                              event.clientY,
                            );
                            setContextMenu({
                              kind: "thread",
                              sessionId: selectedSessionId,
                              threadId: thread.thread_id,
                              currentValue: thread.title ?? "",
                              x: position.x,
                              y: position.y,
                            });
                          }}
                        >
                          <div className="flex items-center gap-2">
                            <div
                              className={cn(
                                "flex size-7 items-center justify-center rounded-lg",
                                isActiveThread
                                  ? "bg-primary text-primary-foreground"
                                  : "bg-muted text-muted-foreground",
                              )}
                            >
                              <MessageSquare className="size-3.5" />
                            </div>
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center justify-between gap-2">
                                <span className="truncate text-sm font-semibold">
                                  {thread.title && thread.title.trim()
                                    ? thread.title
                                    : thread.thread_id}
                                </span>
                                {isActiveThread ? (
                                  <Check className="size-3.5 shrink-0 text-primary" />
                                ) : null}
                              </div>
                              <div className="mt-1 flex items-center gap-3 text-[10px] text-muted-foreground">
                                <span>{thread.turn_count} turns</span>
                                <span>{renderTimestamp(thread.updated_at)}</span>
                              </div>
                            </div>
                          </div>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          </div>

          <div className="bg-muted/10 px-8 py-4 border-t border-muted/60 flex items-center justify-center">
            <p className="text-[10px] text-muted-foreground font-medium uppercase tracking-tighter opacity-50">
              {sessionList.length} 个会话 · 右键可重命名
            </p>
          </div>
        </DialogContent>
      </Dialog>

      {hasMounted && contextMenu
        ? createPortal(
            <div
              ref={contextMenuRef}
              className="fixed z-[70] min-w-40 rounded-xl border border-border bg-background p-1 shadow-2xl"
              style={{ left: contextMenu.x, top: contextMenu.y }}
              onClick={(event) => event.stopPropagation()}
            >
              {contextMenu.kind === "session" ? (
                <>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs hover:bg-muted"
                    onClick={() =>
                      openRenameDialog({
                        kind: "session",
                        sessionId: contextMenu.sessionId,
                        currentValue: contextMenu.currentValue,
                      })
                    }
                  >
                    <Pencil className="size-3.5" />
                    重命名 Session
                  </button>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs text-destructive hover:bg-destructive/10"
                    onClick={() => {
                      setDeleteConfirm(contextMenu.sessionId);
                      setContextMenu(null);
                    }}
                  >
                    <Trash2 className="size-3.5" />
                    删除 Session
                  </button>
                </>
              ) : (
                <>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs hover:bg-muted"
                    onClick={() =>
                      openRenameDialog({
                        kind: "thread",
                        sessionId: contextMenu.sessionId,
                        threadId: contextMenu.threadId,
                        currentValue: contextMenu.currentValue,
                      })
                    }
                  >
                    <Pencil className="size-3.5" />
                    重命名 Thread
                  </button>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs hover:bg-muted"
                    onClick={() => {
                      handleSwitchThread(contextMenu.sessionId, contextMenu.threadId);
                      setContextMenu(null);
                    }}
                  >
                    <MessageSquare className="size-3.5" />
                    切换到此 Thread
                  </button>
                </>
              )}
            </div>,
            document.body,
          )
        : null}

      <Dialog
        open={!!renameTarget}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) {
            setRenameTarget(null);
            setRenameValue("");
          }
        }}
      >
        <DialogContent className="sm:max-w-[420px]">
          <DialogHeader>
            <DialogTitle>
              {renameTarget?.kind === "session" ? "重命名 Session" : "重命名 Thread"}
            </DialogTitle>
            <DialogDescription>
              留空即可恢复为 ID 回退显示。
            </DialogDescription>
          </DialogHeader>
          <Input
            autoFocus
            value={renameValue}
            onChange={(event) => setRenameValue(event.target.value)}
            placeholder={renameTarget?.kind === "session" ? "输入 Session 名称" : "输入 Thread 标题"}
          />
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setRenameTarget(null);
                setRenameValue("");
              }}
            >
              取消
            </Button>
            <Button onClick={() => void handleRenameSubmit()} disabled={renameSaving}>
              {renameSaving ? "保存中..." : "保存"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
