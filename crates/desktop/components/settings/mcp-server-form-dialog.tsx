"use client";

import * as React from "react";
import { Plus, Pencil, Trash2 } from "lucide-react";
import type { McpServerPayload, ServerType } from "@/lib/tauri";
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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface McpServerFormDialogProps {
  server?: McpServerPayload | null;
  onSubmit: (record: McpServerPayload) => Promise<void>;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  trigger?: React.ReactElement | null;
}

interface HeaderEntry {
  key: string;
  value: string;
}

function createDefaultFormData(): McpServerPayload {
  return {
    id: 0,
    name: "",
    display_name: "",
    server_type: "stdio",
    command: "",
    url: "",
    headers: {},
    use_sse: false,
    args: [],
    enabled: true,
  };
}

function normalizeServerType(serverType: string): ServerType {
  return serverType.toLowerCase() === "http" ? "http" : "stdio";
}

function validateName(name: string): string | null {
  if (!name.trim()) {
    return "名称不能为空";
  }
  if (!/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(name)) {
    return "名称只能包含字母、数字、下划线和连字符，必须以字母开头";
  }
  return null;
}

export function McpServerFormDialog({
  server,
  onSubmit,
  open: openProp,
  onOpenChange,
  trigger,
}: McpServerFormDialogProps) {
  const [internalOpen, setInternalOpen] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [nameError, setNameError] = React.useState<string | null>(null);
  const isEditing = !!server;
  const open = openProp ?? internalOpen;

  const [formData, setFormData] = React.useState<McpServerPayload>(
    () => server || createDefaultFormData(),
  );

  // Headers as array for easier UI management
  const [headers, setHeaders] = React.useState<HeaderEntry[]>([]);

  // Sync headers from formData when dialog opens
  React.useEffect(() => {
    if (open) {
      const currentHeaders = formData.headers || {};
      const entries = Object.entries(currentHeaders).map(([key, value]) => ({ key, value }));
      setHeaders(entries);
    }
  }, [open, formData.headers]);

  React.useEffect(() => {
    if (server) {
      setFormData({
        ...server,
        server_type: normalizeServerType(server.server_type),
        use_sse: Boolean(server.use_sse),
      });
    } else {
      setFormData(createDefaultFormData());
    }
    setNameError(null);
  }, [server]);

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (openProp === undefined) {
        setInternalOpen(nextOpen);
      }
      onOpenChange?.(nextOpen);
    },
    [onOpenChange, openProp],
  );

  const handleNameChange = React.useCallback((name: string) => {
    setFormData((prev) => ({ ...prev, name }));
    setNameError(validateName(name));
  }, []);

  const handleServerTypeChange = React.useCallback((server_type: ServerType) => {
    setFormData((prev) => ({ ...prev, server_type }));
  }, []);

  const handleAddHeader = React.useCallback(() => {
    setHeaders((prev) => [...prev, { key: "", value: "" }]);
  }, []);

  const handleRemoveHeader = React.useCallback((index: number) => {
    setHeaders((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handleHeaderChange = React.useCallback((index: number, field: "key" | "value", value: string) => {
    setHeaders((prev) => prev.map((entry, i) => (i === index ? { ...entry, [field]: value } : entry)));
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const error = validateName(formData.name);
    if (error) {
      setNameError(error);
      return;
    }

    // Filter empty headers and convert to Record
    const filteredHeaders: Record<string, string> = {};
    for (const { key, value } of headers) {
      if (key.trim() && value.trim()) {
        filteredHeaders[key.trim()] = value.trim();
      }
    }

    const payload: McpServerPayload = {
      ...formData,
      headers: Object.keys(filteredHeaders).length > 0 ? filteredHeaders : undefined,
    };

    setSaving(true);
    try {
      await onSubmit(payload);
      handleOpenChange(false);
    } catch (err) {
      console.error("Failed to save MCP server:", err);
    } finally {
      setSaving(false);
    }
  };

  const canSubmit =
    formData.display_name.trim() &&
    formData.name.trim() &&
    !nameError &&
    (formData.server_type === "stdio" ? formData.command?.trim() : formData.url?.trim());

  const defaultTrigger = isEditing ? (
    <Button size="sm" variant="outline">
      <Pencil className="h-3 w-3" />
    </Button>
  ) : (
    <Button size="sm">
      <Plus className="h-4 w-4 mr-1" />
      添加 MCP 服务器
    </Button>
  );
  const dialogTrigger = trigger === undefined ? defaultTrigger : trigger;

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      {dialogTrigger ? <DialogTrigger render={dialogTrigger} /> : null}
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {isEditing ? "编辑 MCP 服务器" : "添加 MCP 服务器"}
          </DialogTitle>
          <DialogDescription>
            {isEditing
              ? "更新 MCP 服务器配置。"
              : "配置一个新的 MCP 服务器以连接工具。"}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="display_name">显示名称</Label>
            <Input
              id="display_name"
              value={formData.display_name}
              onChange={(e) =>
                setFormData({ ...formData, display_name: e.target.value })
              }
              placeholder="文件系统工具"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="name">
              名称{" "}
              <span className="text-muted-foreground text-xs">
                (用于工具命名，如 mcp_filesystem_read)
              </span>
            </Label>
            <Input
              id="name"
              value={formData.name}
              onChange={(e) => handleNameChange(e.target.value)}
              placeholder="filesystem"
              required
            />
            {nameError && (
              <p className="text-xs text-destructive">{nameError}</p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="server_type">传输方式</Label>
            <Select
              value={formData.server_type}
              onValueChange={(value) => {
                if (value) handleServerTypeChange(value as ServerType);
              }}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="选择传输方式" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="stdio">标准 I/O (Stdio)</SelectItem>
                <SelectItem value="http">HTTP / SSE</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {formData.server_type === "stdio" ? (
            <div className="space-y-2">
              <Label htmlFor="command">
                命令{" "}
                <span className="text-muted-foreground text-xs">
                  (MCP 服务器启动命令)
                </span>
              </Label>
              <Input
                id="command"
                value={formData.command || ""}
                onChange={(e) =>
                  setFormData({ ...formData, command: e.target.value })
                }
                placeholder="npx -y @modelcontextprotocol/server-filesystem /path/to/directory"
                required
              />
            </div>
          ) : (
            <div className="space-y-2">
              <Label htmlFor="url">
                URL{" "}
                <span className="text-muted-foreground text-xs">
                  (MCP 服务器 SSE 端点)
                </span>
              </Label>
              <Input
                id="url"
                value={formData.url || ""}
                onChange={(e) =>
                  setFormData({ ...formData, url: e.target.value })
                }
                placeholder="http://localhost:3000/sse"
                required
              />
            </div>
          )}
          {formData.server_type === "http" && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="use_sse"
                  checked={formData.use_sse}
                  onChange={(e) =>
                    setFormData({ ...formData, use_sse: e.target.checked })
                  }
                  className="h-4 w-4 rounded border-input"
                />
                <Label htmlFor="use_sse" className="cursor-pointer">
                  使用 SSE
                </Label>
              </div>
              <p className="text-xs text-muted-foreground">
                默认使用普通 HTTP（Streamable HTTP），仅在服务端只支持 SSE 时开启。
              </p>
              <div className="flex items-center justify-between">
                <Label>HTTP Headers</Label>
                <Button type="button" variant="ghost" size="sm" onClick={handleAddHeader}>
                  <Plus className="h-3 w-3 mr-1" />
                  Add Header
                </Button>
              </div>
              {headers.length === 0 ? (
                <p className="text-xs text-muted-foreground">No headers configured</p>
              ) : (
                <div className="space-y-2">
                  {headers.map((header, index) => (
                    <div key={index} className="flex gap-2 items-center">
                      <Input
                        value={header.key}
                        onChange={(e) => handleHeaderChange(index, "key", e.target.value)}
                        placeholder="Authorization"
                        className="flex-1"
                      />
                      <Input
                        value={header.value}
                        onChange={(e) => handleHeaderChange(index, "value", e.target.value)}
                        placeholder="Bearer your_token"
                        className="flex-1"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => handleRemoveHeader(index)}
                      >
                        <Trash2 className="h-3 w-3 text-destructive" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="enabled"
              checked={formData.enabled}
              onChange={(e) =>
                setFormData({ ...formData, enabled: e.target.checked })
              }
              className="h-4 w-4 rounded border-input"
            />
            <Label htmlFor="enabled" className="cursor-pointer">
              启用服务器
            </Label>
          </div>
          <DialogFooter>
            <Button type="submit" disabled={saving || !canSubmit}>
              {saving ? "保存中..." : isEditing ? "更新" : "创建"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
