"use client"

import * as React from "react"
import { Link, useNavigate } from "react-router-dom"
import { Plus, Bot } from "lucide-react"
import { agents, providers, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import {
  AgentCard,
  DeleteConfirmDialog,
} from "@/components/settings"
import { Button } from "@/components/ui/button"

export default function AgentsPage() {
  const navigate = useNavigate()
  const [agentList, setAgentList] = React.useState<AgentRecord[]>([])
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [loading, setLoading] = React.useState(true)
  const [deleteId, setDeleteId] = React.useState<number | null>(null)
  const [deleteLoading, setDeleteLoading] = React.useState(false)

  const loadData = React.useCallback(async () => {
    try {
      const [agentsData, providersData] = await Promise.all([
        agents.list(),
        providers.list(),
      ])
      setAgentList(agentsData)
      setProviderList(providersData)
    } catch (error) {
      console.error("Failed to load data:", error)
    } finally {
      setLoading(false)
    }
  }, [])

  React.useEffect(() => {
    loadData()
  }, [loadData])

  const handleEdit = (id: number) => {
    navigate(`/settings/agents/edit?id=${id}`)
  }

  const handleDelete = async () => {
    if (!deleteId) return
    setDeleteLoading(true)
    try {
      await agents.delete(deleteId)
      setDeleteId(null)
      await loadData()
    } finally {
      setDeleteLoading(false)
    }
  }

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载智能体...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-4 animate-in fade-in duration-500">
      {/* 顶部标题栏 */}
      <div className="flex flex-col gap-3 border-b pb-4 md:flex-row md:items-center md:justify-between">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Bot className="h-4 w-4 text-primary" />
            <h1 className="text-lg font-bold tracking-tight">智能体管理</h1>
          </div>
          <p className="text-muted-foreground text-sm">
            在这里创建、配置和管理您的 AI 智能体及其子智能体。
          </p>
        </div>

        <div className="flex items-center gap-2">
          <Link to="/settings/agents/new">
            <Button size="sm" className="shadow-sm">
              <Plus className="h-4 w-4 mr-1.5" />
              新建智能体
            </Button>
          </Link>
        </div>
      </div>

      {/* 智能体列表 */}
      {agentList.length === 0 ? (
        <div className="flex h-64 flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed bg-muted/20">
          <div className="rounded-full bg-muted p-3">
            <Bot className="h-7 w-7 text-muted-foreground/50" />
          </div>
          <div className="text-center space-y-1">
            <p className="font-medium text-muted-foreground">暂无智能体配置</p>
            <p className="text-xs text-muted-foreground/60">开始创建您的第一个 AI 助手吧</p>
          </div>
          <Link to="/settings/agents/new">
            <Button size="sm" className="px-4">
              <Plus className="h-4 w-4 mr-1.5" />
              立即创建
            </Button>
          </Link>
        </div>
      ) : (
        <div className="grid gap-3">
          {agentList.map((agent) => (
            <AgentCard
              key={agent.id}
              agent={agent}
              providers={providerList}
              onEdit={handleEdit}
              onDelete={(id) => setDeleteId(id)}
            />
          ))}
        </div>
      )}

      {/* 删除确认对话框 */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="确认删除"
        description="此操作将永久删除该智能体及其关联配置，且无法撤销。您确定要继续吗？"
        onConfirm={handleDelete}
        loading={deleteLoading}
      />
    </div>
  )
}
