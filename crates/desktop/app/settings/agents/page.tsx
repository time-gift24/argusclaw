"use client"

import * as React from "react"
import Link from "next/link"
import { useRouter } from "next/navigation"
import { Plus } from "lucide-react"
import { agents, providers, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import {
  AgentCard,
  DeleteConfirmDialog,
} from "@/components/settings"
import { Button } from "@/components/ui/button"

export default function AgentsPage() {
  const router = useRouter()
  const [agentList, setAgentList] = React.useState<AgentRecord[]>([])
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [loading, setLoading] = React.useState(true)
  const [deleteId, setDeleteId] = React.useState<string | null>(null)
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

  const handleEdit = (id: string) => {
    router.push(`/settings/agents/${id}`)
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
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-sm font-semibold">智能体</h1>
          <p className="text-muted-foreground text-xs">
            配置你的 AI 智能体
          </p>
        </div>
        <Link href="/settings/agents/new">
          <Button size="sm">
            <Plus className="h-4 w-4 mr-1" />
            新建智能体
          </Button>
        </Link>
      </div>

      {agentList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground mb-4">暂无智能体配置</p>
          <Link href="/settings/agents/new">
            <Button size="sm">
              <Plus className="h-4 w-4 mr-1" />
              新建智能体
            </Button>
          </Link>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
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

      {/* Delete Confirmation */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="删除智能体"
        description="确定要删除此智能体吗？此操作无法撤销。"
        onConfirm={handleDelete}
        loading={deleteLoading}
      />
    </div>
  )
}
