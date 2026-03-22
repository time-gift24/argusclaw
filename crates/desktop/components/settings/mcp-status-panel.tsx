"use client";

import { useEffect, useState } from "react";
import { mcpServers, type McpServerStatus } from "@/lib/tauri";
import { STATUS_COLORS, STATUS_LABELS } from "@/lib/mcp-status";

interface McpStatusPanelProps {
  serverId: number;
}

export function McpStatusPanel({ serverId }: McpStatusPanelProps) {
  const [status, setStatus] = useState<McpServerStatus>({
    status: "disconnected",
  });
  const [loading, setLoading] = useState(false);

  async function loadStatus() {
    try {
      const all = await mcpServers.getStatuses();
      setStatus(all[serverId] ?? { status: "disconnected" });
    } catch {
      // keep current status on error
    }
  }

  async function handleRefresh() {
    setLoading(true);
    setStatus((prev) => ({ ...prev, status: "connecting" }));
    try {
      const s = await mcpServers.testServer(serverId);
      setStatus(s);
    } catch (e) {
      console.error("Test failed", e);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadStatus();
    const interval = setInterval(() => void loadStatus(), 30000);
    return () => clearInterval(interval);
  }, [serverId]);

  const tools = status.status === "connected" ? status.tools : [];
  const showTools = tools.length > 0;

  return (
    <div className="rounded-lg border p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">连接状态</h3>
        <button
          onClick={() => void handleRefresh()}
          disabled={loading || status.status === "connecting"}
          className="text-xs px-2 py-1 rounded border hover:bg-muted disabled:opacity-50"
        >
          {status.status === "connecting" ? "测试中..." : "刷新"}
        </button>
      </div>

      <span
        className={`inline-block text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_COLORS[status.status]}`}
      >
        {STATUS_LABELS[status.status]}
      </span>

      {status.status === "failed" && (
        <p className="text-xs text-red-600 mt-1">{status.error}</p>
      )}

      {showTools && (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">
            可用工具 ({tools.length})
          </p>
          <div className="flex flex-wrap gap-1 max-h-40 overflow-y-auto">
            {tools.slice(0, 10).map((tool) => (
              <span
                key={tool}
                className="bg-muted text-muted-foreground text-xs px-2 py-0.5 rounded-full"
              >
                {tool}
              </span>
            ))}
            {tools.length > 10 && (
              <span className="text-xs text-muted-foreground px-1 py-0.5">
                +{tools.length - 10} 更多
              </span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
