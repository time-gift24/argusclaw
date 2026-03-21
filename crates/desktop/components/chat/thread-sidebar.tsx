"use client";

import * as React from "react";
import {
  AlertTriangle,
  ChevronDownIcon,
  MoreHorizontalIcon,
  PencilIcon,
  PlusIcon,
  Trash2Icon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { chat, sessions as sessionsApi } from "@/lib/tauri";
import { useChatStore } from "@/lib/chat-store";
import { useThreadListStore } from "@/lib/thread-list-store";

function formatRelativeTime(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);

  if (diffSec < 60) return "刚刚";
  if (diffMin < 60) return `${diffMin} 分钟前`;
  if (diffHour < 24) return `${diffHour} 小时前`;
  if (diffDay < 7) return `${diffDay} 天前`;
  return date.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

interface DeleteDialogState {
  open: boolean;
  sessionId: number | null;
  sessionName: string;
}

export function ThreadSidebar() {
  const { sessions, activeSessionId, fetchSessions, deleteSession, updateTitle, selectSession, cleanup } =
    useThreadListStore();
  const { selectedTemplateId, selectedProviderPreferenceId, activateSession, removeSession } = useChatStore();

  const [editingId, setEditingId] = React.useState<number | null>(null);
  const [editingValue, setEditingValue] = React.useState("");
  const [deleteDialog, setDeleteDialog] = React.useState<DeleteDialogState>({
    open: false,
    sessionId: null,
    sessionName: "",
  });
  const [isCreating, setIsCreating] = React.useState(false);

  // Fetch sessions on mount
  React.useEffect(() => {
    void fetchSessions();
  }, [fetchSessions]);

  // Select most recent session on mount if none active
  React.useEffect(() => {
    if (sessions.length > 0 && activeSessionId === null) {
      // Pick the most recently updated session
      const sorted = [...sessions].sort(
        (a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime(),
      );
      selectSession(sorted[0].id);
    }
  }, [sessions, activeSessionId, selectSession]);

  const handleSelectSession = async (sessionId: number) => {
    selectSession(sessionId);
    // The ChatScreen will use activeSessionId to determine which session to show
  };

  const handleCreateThread = async () => {
    if (!selectedTemplateId) return;
    setIsCreating(true);
    try {
      const session = await chat.createChatSession(selectedTemplateId, selectedProviderPreferenceId);
      await fetchSessions();
      selectSession(session.session_id);
    } catch (error) {
      console.error("Failed to create thread:", error);
    } finally {
      setIsCreating(false);
    }
  };

  const handleStartEdit = (sessionId: number, currentName: string) => {
    setEditingId(sessionId);
    setEditingValue(currentName);
  };

  const handleSaveEdit = async () => {
    if (editingId === null) return;
    const trimmed = editingValue.trim();
    if (trimmed) {
      try {
        await updateTitle(editingId, trimmed);
      } catch (error) {
        console.error("Failed to update title:", error);
      }
    }
    setEditingId(null);
    setEditingValue("");
  };

  const handleEditKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      void handleSaveEdit();
    } else if (e.key === "Escape") {
      setEditingId(null);
      setEditingValue("");
    }
  };

  const handleDeleteConfirm = async () => {
    if (deleteDialog.sessionId === null) return;
    const deletedId = deleteDialog.sessionId;
    const wasActive = deletedId === activeSessionId;
    const wasLast = sessions.length === 1;
    try {
      await deleteSession(deletedId);
      // Always clean up the chat store entry for the deleted session
      removeSession(deletedId);
      if (wasLast) {
        // Spec: immediately create and activate a replacement thread
        setDeleteDialog({ open: false, sessionId: null, sessionName: "" });
        void handleCreateThread();
      } else if (wasActive) {
        const remaining = sessions.filter((s) => s.id !== deletedId);
        const sorted = [...remaining].sort(
          (a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime(),
        );
        selectSession(sorted[0].id);
        setDeleteDialog({ open: false, sessionId: null, sessionName: "" });
      } else {
        setDeleteDialog({ open: false, sessionId: null, sessionName: "" });
      }
    } catch (error) {
      console.error("Failed to delete session:", error);
    }
  };

  const handleCleanup = async () => {
    try {
      await cleanup();
    } catch (error) {
      console.error("Cleanup failed:", error);
    }
  };

  return (
    <>
      <div className="flex h-full w-64 shrink-0 flex-col border-r bg-background">
        {/* Header */}
        <div className="flex items-center justify-between px-3 py-3">
          <span className="text-sm font-medium">对话</span>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => void handleCreateThread()}
            disabled={isCreating || !selectedTemplateId}
            title="新建对话"
          >
            <PlusIcon className="h-4 w-4" />
          </Button>
        </div>

        <Separator />

        {/* Session list */}
        <div className="flex-1 overflow-y-auto py-2">
          {sessions.length === 0 ? (
            <div className="px-3 py-8 text-center text-xs text-muted-foreground">
              暂无对话
            </div>
          ) : (
            <ul className="space-y-0.5 px-1">
              {sessions
                .slice()
                .sort(
                  (a, b) =>
                    new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime(),
                )
                .map((session) => (
                  <li key={session.id}>
                    <SessionItem
                      id={session.id}
                      name={session.name}
                      updatedAt={session.updated_at}
                      isActive={session.id === activeSessionId}
                      isEditing={editingId === session.id}
                      editingValue={editingValue}
                      onSelect={() => void handleSelectSession(session.id)}
                      onStartEdit={() => handleStartEdit(session.id, session.name)}
                      onEditChange={setEditingValue}
                      onEditSave={handleSaveEdit}
                      onEditKeyDown={handleEditKeyDown}
                      onEditBlur={handleSaveEdit}
                      onDelete={() =>
                        setDeleteDialog({ open: true, sessionId: session.id, sessionName: session.name })
                      }
                    />
                  </li>
                ))}
            </ul>
          )}
        </div>

        <Separator />

        {/* Footer */}
        <div className="px-3 py-2">
          <Button
            variant="ghost"
            size="sm"
            className="w-full justify-start gap-2 text-xs text-muted-foreground"
            onClick={() => void handleCleanup()}
          >
            <Trash2Icon className="h-3.5 w-3.5" />
            清理 14 天前的对话
          </Button>
        </div>
      </div>

      {/* Delete confirmation dialog */}
      <Dialog
        open={deleteDialog.open}
        onOpenChange={(open) =>
          setDeleteDialog((s) => ({ ...s, open }))
        }
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <div className="flex items-center gap-2 text-destructive">
              <AlertTriangle className="h-5 w-5" />
              <DialogTitle>删除对话</DialogTitle>
            </div>
            <DialogDescription>
              确定要删除「{deleteDialog.sessionName}」吗？此操作无法撤销。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              variant="outline"
              onClick={() => setDeleteDialog((s) => ({ ...s, open: false }))}
            >
              取消
            </Button>
            <Button variant="destructive" onClick={() => void handleDeleteConfirm()}>
              删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

interface SessionItemProps {
  id: number;
  name: string;
  updatedAt: string;
  isActive: boolean;
  isEditing: boolean;
  editingValue: string;
  onSelect: () => void;
  onStartEdit: () => void;
  onEditChange: (value: string) => void;
  onEditSave: () => void;
  onEditKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void;
  onEditBlur: () => void;
  onDelete: () => void;
}

function SessionItem({
  name,
  updatedAt,
  isActive,
  isEditing,
  editingValue,
  onSelect,
  onStartEdit,
  onEditChange,
  onEditSave,
  onEditKeyDown,
  onEditBlur,
  onDelete,
}: SessionItemProps) {
  return (
    <div
      className={cn(
        "group relative flex items-center gap-1 rounded-md px-2 py-1.5 text-xs transition-colors",
        isActive
          ? "bg-accent text-accent-foreground"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      <button
        className="flex flex-1 items-center gap-2 text-left"
        onClick={onSelect}
      >
        <ChevronDownIcon
          className={cn(
            "h-3 w-3 shrink-0 rotate-[-90deg] transition-transform",
          )}
        />
        <span className="truncate flex-1">{name}</span>
      </button>

      {isEditing ? (
        <Input
          className="h-5 w-32 text-xs"
          value={editingValue}
          onChange={(e) => onEditChange(e.target.value)}
          onKeyDown={onEditKeyDown}
          onBlur={onEditBlur}
          autoFocus
        />
      ) : (
        <>
          <span className="ml-auto shrink-0 pr-1 text-[10px] tabular-nums">
            {formatRelativeTime(updatedAt)}
          </span>
          <DropdownMenu>
            <DropdownMenuTrigger
            className="absolute right-1 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100"
            onClick={(e) => e.stopPropagation()}
          >
            <MoreHorizontalIcon className="h-3.5 w-3.5" />
          </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  onStartEdit();
                }}
              >
                <PencilIcon className="h-3.5 w-3.5" />
                重命名
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete();
                }}
              >
                <Trash2Icon className="h-3.5 w-3.5" />
                删除
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      )}
    </div>
  );
}
