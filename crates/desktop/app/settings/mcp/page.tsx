"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { Plus, Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { mcpServers, type McpServerConfig, type McpServerStatus } from "@/lib/tauri";
import { STATUS_COLORS, STATUS_LABELS } from "@/lib/mcp-status";
import { DeleteConfirmDialog } from "@/components/settings";

export default function McpServersPage() {
  const router = useRouter();
  const [serverList, setServerList] = React.useState<McpServerConfig[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [deleteId, setDeleteId] = React.useState<number | null>(null);
  const [deleteLoading, setDeleteLoading] = React.useState(false);
  const [statuses, setStatuses] = React.useState<Record<number, McpServerStatus>>({});

  React.useEffect(() => {
    async function loadStatuses() {
      try {
        const s = await mcpServers.getStatuses();
        setStatuses(s);
      } catch (e) {
        console.error("Failed to load MCP statuses", e);
      }
    }
    loadStatuses();
    const interval = setInterval(loadStatuses, 30000);
    return () => clearInterval(interval);
  }, []);

  const loadServers = React.useCallback(async () => {
    try {
      const data = await mcpServers.list();
      setServerList(data);
    } catch (error) {
      console.error("Failed to load MCP servers:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  React.useEffect(() => {
    loadServers();
  }, [loadServers]);

  const handleDelete = async () => {
    if (deleteId === null) return;
    setDeleteLoading(true);
    try {
      await mcpServers.delete(deleteId);
      setDeleteId(null);
      await loadServers();
    } finally {
      setDeleteLoading(false);
    }
  };

  async function handleTest(id: number) {
    setStatuses((prev) => ({ ...prev, [id]: { status: "connecting" as const, tools: [], error: undefined } }));
    try {
      const status = await mcpServers.testServer(id);
      setStatuses((prev) => ({ ...prev, [id]: status }));
    } catch (e) {
      console.error("Test failed", e);
    }
  }

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
        <div>
          <h1 className="text-sm font-semibold">MCP 服务器</h1>
          <p className="text-muted-foreground text-xs">
            配置你的 MCP 服务器连接
          </p>
        </div>
        <Button size="sm" onClick={() => router.push("/settings/mcp/new")}>
          <Plus className="h-4 w-4 mr-1" />
          新建
        </Button>
      </div>

      {serverList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground mb-4">暂无 MCP 服务器</p>
          <Button size="sm" onClick={() => router.push("/settings/mcp/new")}>
            <Plus className="h-4 w-4 mr-1" />
            新建
          </Button>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {serverList.map((server) => (
            <Card key={server.id}>
              <CardHeader className="pb-3">
                <div className="flex items-center justify-between">
                  <CardTitle className="text-base flex items-center gap-2">
                    <Server className="h-5 w-5 text-muted-foreground" />
                    <span>{server.display_name}</span>
                  </CardTitle>
                  <Badge variant={server.enabled ? "default" : "secondary"}>
                    {server.enabled ? "已启用" : "已禁用"}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="space-y-2 text-sm">
                <div className="flex justify-between">
                  <span className="text-muted-foreground">名称:</span>
                  <span className="font-mono text-xs">{server.name}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">类型:</span>
                  <Badge variant="outline" className="font-mono text-xs">
                    {server.server_type}
                  </Badge>
                </div>
                {server.server_type === "http" && server.url && (
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">URL:</span>
                    <span className="font-mono text-xs break-all">{server.url}</span>
                  </div>
                )}
                {server.server_type === "stdio" && server.command && (
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">命令:</span>
                    <span className="font-mono text-xs">{server.command}</span>
                  </div>
                )}

                {/* Connection Status */}
                {(() => {
                  const s = statuses[server.id];
                  if (!s) return null;
                  return (
                    <div className="mt-3 space-y-2">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_COLORS[s.status]}`}>
                          {STATUS_LABELS[s.status]}
                        </span>
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={s.status === "connecting"}
                          onClick={() => void handleTest(server.id)}
                        >
                          {s.status === "connecting" ? "测试中..." : "测试"}
                        </Button>
                      </div>

                      {s.status === "failed" && (
                        <p className="text-xs text-red-600">{s.error}</p>
                      )}

                      {s.status === "connected" && s.tools.length > 0 && (
                        <div className="flex flex-wrap gap-1 mt-1">
                          {s.tools.slice(0, 10).map((tool) => (
                            <span
                              key={tool}
                              className="bg-muted text-muted-foreground text-xs px-2 py-0.5 rounded-full"
                            >
                              {tool}
                            </span>
                          ))}
                          {s.tools.length > 10 && (
                            <span className="text-xs text-muted-foreground px-1 py-0.5">
                              +{s.tools.length - 10} 更多
                            </span>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })()}
              </CardContent>
              <CardFooter className="flex gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => router.push(`/settings/mcp/${server.id}`)}
                >
                  编辑
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={() => setDeleteId(server.id)}
                >
                  删除
                </Button>
              </CardFooter>
            </Card>
          ))}
        </div>
      )}

      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="删除 MCP 服务器"
        description="确定要删除此 MCP 服务器吗？此操作无法撤销。"
        onConfirm={handleDelete}
        loading={deleteLoading}
      />
    </div>
  );
}
