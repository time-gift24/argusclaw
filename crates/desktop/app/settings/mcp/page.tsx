"use client"

import * as React from "react"
import { useRouter } from "next/navigation"
import { Plus, Server } from "lucide-react"

import { mcp, type McpConnectionTestResult, type McpServerRecord } from "@/lib/tauri"
import { DeleteConfirmDialog, McpCard, McpTestDialog } from "@/components/settings"
import { Button } from "@/components/ui/button"

export default function McpSettingsPage() {
  const router = useRouter()
  const [serverList, setServerList] = React.useState<McpServerRecord[]>([])
  const [loading, setLoading] = React.useState(true)
  const [deleteId, setDeleteId] = React.useState<number | null>(null)
  const [deleteLoading, setDeleteLoading] = React.useState(false)
  const [testingServerId, setTestingServerId] = React.useState<number | null>(null)
  const [testResultsByServerId, setTestResultsByServerId] = React.useState<Record<number, McpConnectionTestResult>>({})
  const [testDialogOpen, setTestDialogOpen] = React.useState(false)
  const [testDialogServerId, setTestDialogServerId] = React.useState<number | null>(null)

  const loadData = React.useCallback(async () => {
    try {
      const data = await mcp.listServers()
      setServerList(data)
    } catch (error) {
      console.error("Failed to load MCP servers:", error)
    } finally {
      setLoading(false)
    }
  }, [])

  React.useEffect(() => {
    void loadData()
  }, [loadData])

  const handleDelete = async () => {
    if (deleteId === null) return
    setDeleteLoading(true)
    try {
      await mcp.deleteServer(deleteId)
      setDeleteId(null)
      setTestResultsByServerId((prev) => {
        const next = { ...prev }
        delete next[deleteId]
        return next
      })
      await loadData()
    } finally {
      setDeleteLoading(false)
    }
  }

  const handleTestConnection = async (serverId: number) => {
    setTestDialogServerId(serverId)
    setTestDialogOpen(true)
    setTestingServerId(serverId)
    try {
      const result = await mcp.testConnection(serverId)
      setTestResultsByServerId((prev) => ({ ...prev, [serverId]: result }))
      await loadData()
    } catch (error) {
      setTestResultsByServerId((prev) => ({
        ...prev,
        [serverId]: {
          status: "failed",
          checked_at: new Date().toISOString(),
          latency_ms: 0,
          discovered_tools: [],
          message: error instanceof Error ? error.message : String(error),
        },
      }))
      console.error("Failed to test MCP server:", error)
    } finally {
      setTestingServerId((current) => (current === serverId ? null : current))
    }
  }

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载 MCP 配置...</div>
      </div>
    )
  }

  const activeTestServer = serverList.find((server) => server.id === testDialogServerId) ?? null
  const activeTestResult =
    testDialogServerId === null ? null : (testResultsByServerId[testDialogServerId] ?? null)

  return (
    <div className="w-full space-y-6 animate-in fade-in duration-500">
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between border-b pb-6">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Server className="h-5 w-5 text-primary" />
            <h1 className="text-xl font-bold tracking-tight">MCP 配置</h1>
          </div>
          <p className="text-muted-foreground text-sm">
            管理 MCP server 配置、连接测试结果和工具发现快照。
          </p>
        </div>
        <Button size="sm" onClick={() => router.push("/settings/mcp/new")} className="h-9 shadow-sm">
          <Plus className="h-4 w-4 mr-1.5" />
          添加 MCP Server
        </Button>
      </div>

      {serverList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-80 border-2 border-dashed rounded-2xl bg-muted/20 gap-4">
          <div className="bg-muted p-4 rounded-full">
            <Server className="h-8 w-8 text-muted-foreground/50" />
          </div>
          <div className="text-center space-y-1">
            <p className="font-medium text-muted-foreground">暂无 MCP Server</p>
            <p className="text-xs text-muted-foreground/60">添加一个 stdio、http 或 sse MCP 端点来供智能体使用</p>
          </div>
          <Button size="sm" onClick={() => router.push("/settings/mcp/new")} className="px-6">
            <Plus className="h-4 w-4 mr-1.5" />
            立即添加
          </Button>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {serverList.map((server) => {
            const serverId = server.id ?? 0
            const result = testResultsByServerId[serverId] ?? null
            return (
              <McpCard
                key={serverId}
                server={server}
                latestResult={result}
                testing={testingServerId === serverId}
                onEdit={() => router.push(`/settings/mcp/edit?id=${serverId}`)}
                onTest={() => void handleTestConnection(serverId)}
                onDelete={() => setDeleteId(serverId)}
                onViewResult={() => {
                  setTestDialogServerId(serverId)
                  setTestDialogOpen(true)
                }}
              />
            )
          })}
        </div>
      )}

      <DeleteConfirmDialog
        open={deleteId !== null}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="确认删除 MCP Server"
        description="此操作将删除该 MCP server 配置及其最近工具快照，且无法撤销。"
        onConfirm={handleDelete}
        loading={deleteLoading}
      />

      <McpTestDialog
        open={testDialogOpen}
        onOpenChange={setTestDialogOpen}
        result={activeTestResult}
        discoveredTools={activeTestResult?.discovered_tools ?? []}
        testing={testDialogOpen && testingServerId === testDialogServerId}
        serverName={activeTestServer?.display_name ?? ""}
        onRetest={() => {
          if (testDialogServerId !== null) {
            void handleTestConnection(testDialogServerId)
          }
        }}
      />
    </div>
  )
}
