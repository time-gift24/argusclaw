"use client";

import * as React from "react";
import { mcpServers, type McpServerSummary, type McpServerPayload, type ConnectionTestResult } from "@/lib/tauri";
import {
  McpServerCard,
  McpServerFormDialog,
  DeleteConfirmDialog,
} from "@/components/settings";

export default function McpServersPage() {
  const [serverList, setServerList] = React.useState<McpServerSummary[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [deleteId, setDeleteId] = React.useState<number | null>(null);
  const [deleteLoading, setDeleteLoading] = React.useState(false);
  const [editingServer, setEditingServer] = React.useState<McpServerPayload | null>(null);
  const [addDialogOpen, setAddDialogOpen] = React.useState(false);
  const [testResultsByServerId, setTestResultsByServerId] = React.useState<
    Record<number, ConnectionTestResult>
  >({});
  const [testingServerId, setTestingServerId] = React.useState<number | null>(null);

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
      setTestResultsByServerId((current) => {
        const next = { ...current };
        delete next[deleteId];
        return next;
      });
      setDeleteId(null);
      await loadServers();
    } finally {
      setDeleteLoading(false);
    }
  };

  const handleUpsert = async (record: McpServerPayload) => {
    await mcpServers.upsert(record);
    await loadServers();
    setEditingServer(null);
  };

  const runConnectionTest = React.useCallback(
    async (id: number) => {
      setTestingServerId(id);
      try {
        const result = await mcpServers.testConnection(id);
        setTestResultsByServerId((current) => ({ ...current, [id]: result }));
      } catch (error) {
        const fallbackResult: ConnectionTestResult = {
          success: false,
          tool_count: 0,
          error_message: error instanceof Error ? error.message : String(error),
        };
        setTestResultsByServerId((current) => ({
          ...current,
          [id]: fallbackResult,
        }));
        console.error("Failed to test MCP server connection:", error);
      } finally {
        setTestingServerId((current) => (current === id ? null : current));
      }
    },
    [],
  );

  const handleTestConnection = React.useCallback(
    (id: number) => {
      void runConnectionTest(id);
    },
    [runConnectionTest],
  );

  const handleEdit = React.useCallback(
    async (id: number) => {
      const server = await mcpServers.get(id);
      if (server) {
        setEditingServer(server);
      }
    },
    [],
  );

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">Loading MCP servers...</div>
      </div>
    );
  }

  return (
    <div className="w-full space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-sm font-semibold">MCP 服务器</h1>
          <p className="text-muted-foreground text-xs">
            配置 MCP 服务器以扩展工具能力
          </p>
        </div>
        <McpServerFormDialog
          open={addDialogOpen}
          onOpenChange={setAddDialogOpen}
          onSubmit={handleUpsert}
        />
      </div>

      {serverList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground mb-4">暂无 MCP 服务器配置</p>
          <McpServerFormDialog
            open={addDialogOpen}
            onOpenChange={setAddDialogOpen}
            onSubmit={handleUpsert}
          />
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {serverList.map((server) => (
            <McpServerCard
              key={server.id}
              server={server}
              onEdit={handleEdit}
              onDelete={(id) => setDeleteId(id)}
              onTestConnection={handleTestConnection}
              testResult={testResultsByServerId[server.id]}
              isTesting={testingServerId === server.id}
            />
          ))}
        </div>
      )}

      {/* Edit Dialog */}
      <McpServerFormDialog
        server={editingServer}
        open={!!editingServer}
        onOpenChange={(open) => !open && setEditingServer(null)}
        onSubmit={handleUpsert}
      />

      {/* Delete Confirmation */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="删除 MCP 服务器"
        description="确定要删除此 MCP 服务器吗？删除后将无法使用该服务器提供的工具。"
        onConfirm={handleDelete}
        loading={deleteLoading}
      />
    </div>
  );
}
