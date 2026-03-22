"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { Save, Plus, X } from "lucide-react";
import { mcpServers, type McpServerConfig } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { useToast } from "@/components/ui/toast";

interface McpServerEditorProps {
  serverId?: number;
  rightPanel?: React.ReactNode;
}

function createDefaultFormData(): Omit<McpServerConfig, "id"> {
  return {
    name: "",
    display_name: "",
    server_type: "stdio",
    url: "",
    headers: {},
    command: "",
    args: [],
    enabled: true,
  };
}

export function McpServerEditor({ serverId, rightPanel }: McpServerEditorProps) {
  const router = useRouter();
  const { addToast } = useToast();
  const isEditing = !!serverId;

  const [loading, setLoading] = React.useState(isEditing);
  const [saving, setSaving] = React.useState(false);
  const [formData, setFormData] = React.useState(createDefaultFormData);
  const [headersExpanded, setHeadersExpanded] = React.useState(false);
  const [newHeaderName, setNewHeaderName] = React.useState("");
  const [newHeaderValue, setNewHeaderValue] = React.useState("");

  // Load server data if editing
  React.useEffect(() => {
    const loadData = async () => {
      if (serverId) {
        try {
          const server = await mcpServers.get(serverId);
          if (server) {
            setFormData({
              name: server.name,
              display_name: server.display_name,
              server_type: server.server_type,
              url: server.url || "",
              headers: server.headers || {},
              command: server.command || "",
              args: server.args || [],
              enabled: server.enabled,
            });
          }
        } catch (error) {
          console.error("Failed to load MCP server:", error);
        } finally {
          setLoading(false);
        }
      } else {
        setLoading(false);
      }
    };
    loadData();
  }, [serverId]);

  const canSave = Boolean(formData.display_name.trim() && formData.name.trim());

  const handleSubmit = async () => {
    if (!canSave) return;

    setSaving(true);
    try {
      const config: McpServerConfig = {
        id: serverId || 0,
        name: formData.name.trim(),
        display_name: formData.display_name.trim(),
        server_type: formData.server_type,
        url: formData.server_type === "http" ? formData.url : undefined,
        headers: formData.server_type === "http" && Object.keys(formData.headers || {}).length > 0
          ? formData.headers
          : undefined,
        command: formData.server_type === "stdio" ? formData.command : undefined,
        args: formData.server_type === "stdio" && (formData.args?.length ?? 0) > 0
          ? formData.args
          : undefined,
        enabled: formData.enabled,
      };
      await mcpServers.upsert(config);
      addToast("success", isEditing ? "MCP 服务器已更新" : "MCP 服务器已创建");
      router.push("/settings/mcp");
    } catch (error) {
      console.error("Failed to save MCP server:", error);
      addToast("error", "保存失败，请重试");
    } finally {
      setSaving(false);
    }
  };

  const handleAddHeader = () => {
    const name = newHeaderName.trim();
    const value = newHeaderValue.trim();
    if (!name) return;
    const headers = formData.headers ?? {};
    if (headers[name] !== undefined) return;
    setFormData({
      ...formData,
      headers: { ...headers, [name]: value },
    });
    setNewHeaderName("");
    setNewHeaderValue("");
  };

  const handleRemoveHeader = (name: string) => {
    setFormData((prev) => {
      const headers = prev.headers ?? {};
      const next = { ...headers };
      delete next[name];
      return { ...prev, headers: next };
    });
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    );
  }

  return (
    <div className="w-full space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-sm font-semibold">
          {isEditing ? "编辑 MCP 服务器" : "新建 MCP 服务器"}
        </h1>
        <Button size="sm" onClick={handleSubmit} disabled={saving || !canSave}>
          <Save className="h-4 w-4 mr-1" />
          {saving ? "保存中..." : "保存"}
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-6">
        {/* Left: Basic Info */}
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="display_name">显示名称</Label>
            <Input
              id="display_name"
              value={formData.display_name}
              onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
              placeholder="我的 MCP 服务器"
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="name">名称 (唯一标识)</Label>
            <Input
              id="name"
              value={formData.name}
              onChange={(e) => setFormData({ ...formData, name: e.target.value })}
              placeholder="my-mcp-server"
              required
            />
            <p className="text-[11px] text-muted-foreground">
              用于程序识别的唯一名称，只能包含字母、数字和连字符
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="server_type">服务器类型</Label>
            <div className="flex gap-4">
              <div className="flex items-center gap-2">
                <input
                  type="radio"
                  id="type_stdio"
                  name="server_type"
                  value="stdio"
                  checked={formData.server_type === "stdio"}
                  onChange={() => setFormData({ ...formData, server_type: "stdio" })}
                />
                <Label htmlFor="type_stdio" className="cursor-pointer">
                  STDIO
                </Label>
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="radio"
                  id="type_http"
                  name="server_type"
                  value="http"
                  checked={formData.server_type === "http"}
                  onChange={() => setFormData({ ...formData, server_type: "http" })}
                />
                <Label htmlFor="type_http" className="cursor-pointer">
                  HTTP
                </Label>
              </div>
            </div>
          </div>

          {formData.server_type === "stdio" ? (
            <>
              <div className="space-y-2">
                <Label htmlFor="command">命令</Label>
                <Input
                  id="command"
                  value={formData.command}
                  onChange={(e) => setFormData({ ...formData, command: e.target.value })}
                  placeholder="npx"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="args">参数</Label>
                <Input
                  id="args"
                  value={formData.args?.join(" ") || ""}
                  onChange={(e) => {
                    const args = e.target.value.trim() ? e.target.value.trim().split(/\s+/) : [];
                    setFormData({ ...formData, args });
                  }}
                  placeholder="--flag value"
                />
                <p className="text-[11px] text-muted-foreground">
                  用空格分隔多个参数
                </p>
              </div>
            </>
          ) : (
            <>
              <div className="space-y-2">
                <Label htmlFor="url">URL</Label>
                <Input
                  id="url"
                  value={formData.url}
                  onChange={(e) => setFormData({ ...formData, url: e.target.value })}
                  placeholder="https://localhost:3000"
                />
              </div>

              {/* Headers Section */}
              <div className="space-y-2 pt-4 border-t">
                <button
                  type="button"
                  className="flex items-center gap-2 text-sm font-medium w-full"
                  onClick={() => setHeadersExpanded((v) => !v)}
                >
                  <span className="text-xs text-muted-foreground">▸</span>
                  HTTP Headers
                </button>
                {headersExpanded && (
                  <div className="space-y-2 pl-3">
                    <div className="flex gap-2">
                      <Input
                        value={newHeaderName}
                        onChange={(e) => setNewHeaderName(e.target.value)}
                        placeholder="Header 名称"
                        className="text-sm"
                      />
                      <Input
                        value={newHeaderValue}
                        onChange={(e) => setNewHeaderValue(e.target.value)}
                        placeholder="Header 值"
                        className="text-sm"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => void handleAddHeader()}
                        disabled={!newHeaderName.trim()}
                      >
                        <Plus className="h-4 w-4" />
                      </Button>
                    </div>
                    {Object.keys(formData.headers ?? {}).length > 0 && (
                      <div className="space-y-1">
                        {Object.entries(formData.headers ?? {}).map(([name, value]) => (
                          <div key={name} className="flex items-center gap-2 text-xs">
                            <Badge variant="secondary" className="shrink-0 font-mono">
                              {name}
                            </Badge>
                            <span className="truncate text-muted-foreground font-mono flex-1 min-w-0">
                              {value || <span className="italic opacity-40">(空)</span>}
                            </span>
                            <button
                              type="button"
                              className="shrink-0 hover:text-destructive"
                              onClick={() => handleRemoveHeader(name)}
                            >
                              <X className="h-3 w-3" />
                            </button>
                          </div>
                        ))}
                      </div>
                    )}
                    <p className="text-[11px] text-muted-foreground">
                      用于向 MCP 服务器发送自定义 HTTP Header
                    </p>
                  </div>
                )}
              </div>
            </>
          )}

          <div className="flex items-center gap-2 pt-4 border-t">
            <Checkbox
              id="enabled"
              checked={formData.enabled}
              onCheckedChange={(checked) => setFormData({ ...formData, enabled: !!checked })}
            />
            <Label htmlFor="enabled" className="cursor-pointer">
              启用此服务器
            </Label>
          </div>
        </div>

        {/* Right: Info or Status Panel */}
        <div className="space-y-4">
          {rightPanel ?? (
            <div className="rounded-lg border bg-muted/30 p-4">
              <h3 className="text-sm font-medium mb-2">服务器类型说明</h3>
              {formData.server_type === "stdio" ? (
                <div className="text-xs text-muted-foreground space-y-2">
                  <p>
                    <strong>STDIO</strong> 类型的服务器通过标准输入/输出与客户端通信。
                  </p>
                  <p>
                    适用于本地运行的 MCP 服务器，如官方 CLI 工具或自定义服务。
                  </p>
                  <p className="font-mono bg-muted p-2 rounded mt-2">
                    npx @anthropic/mcp-server AnthropicClaude
                  </p>
                </div>
              ) : (
                <div className="text-xs text-muted-foreground space-y-2">
                  <p>
                    <strong>HTTP</strong> 类型的服务器通过 HTTP API 与客户端通信。
                  </p>
                  <p>
                    适用于远程运行的 MCP 服务器，需要提供完整的 URL 地址。
                  </p>
                  <p className="font-mono bg-muted p-2 rounded mt-2">
                    https://api.example.com/mcp
                  </p>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
