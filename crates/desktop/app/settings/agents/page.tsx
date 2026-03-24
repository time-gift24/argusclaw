"use client"

import * as React from "react"
import Link from "next/link"
import { useRouter } from "next/navigation"
import { Plus, ChevronDown } from "lucide-react"
import { agents, providers, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import {
  AgentCard,
  DeleteConfirmDialog,
} from "@/components/settings"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

export default function AgentsPage() {
  const router = useRouter()
  const [agentList, setAgentList] = React.useState<AgentRecord[]>([])
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [loading, setLoading] = React.useState(true)
  const [showSubagents, setShowSubagents] = React.useState(false)
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
    router.push(`/settings/agents/edit?id=${id}`)
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

  // Separate parent agents (standard agents without parent) from subagents
  const parentAgents = agentList.filter(
    (agent) => !agent.parent_agent_id && agent.agent_type !== "subagent"
  )
  const subagents = agentList.filter(
    (agent) => agent.parent_agent_id || agent.agent_type === "subagent"
  )

  // Build displayed list: parents first, then subagents (if shown)
  const displayedAgents = React.useMemo(() => {
    const result: Array<{ agent: AgentRecord; isSubagent: boolean }> = []
    for (const agent of parentAgents) {
      result.push({ agent, isSubagent: false })
      if (showSubagents) {
        const agentSubagents = subagents.filter((s) => s.parent_agent_id === agent.id)
        for (const sub of agentSubagents) {
          result.push({ agent: sub, isSubagent: true })
        }
      }
    }
    return result
  }, [parentAgents, subagents, showSubagents])

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
        <div className="flex items-center gap-4">
          <div>
            <h1 className="text-sm font-semibold">智能体</h1>
            <p className="text-muted-foreground text-xs">
              配置你的 AI 智能体
            </p>
          </div>
          {subagents.length > 0 && (
            <button
              onClick={() => setShowSubagents(!showSubagents)}
              className={`text-xs px-2 py-1 rounded border cursor-pointer transition-colors ${
                showSubagents
                  ? "bg-primary text-primary-foreground border-primary"
                  : "bg-muted text-muted-foreground border-transparent"
              }`}
            >
              {showSubagents ? "隐藏子智能体" : `显示子智能体 (${subagents.length})`}
            </button>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Link href="/settings/agents/new">
            <Button size="sm">
              <Plus className="h-4 w-4 mr-1" />
              新建智能体
            </Button>
          </Link>
          {parentAgents.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger render={
                <Button size="sm" variant="outline">
                  <Plus className="h-4 w-4 mr-1" />
                  新建子智能体
                  <ChevronDown className="h-3 w-3 ml-1" />
                </Button>
              } />
              <DropdownMenuContent align="end">
                {parentAgents.map((agent) => (
                  <DropdownMenuItem
                    key={agent.id}
                    onClick={() => router.push(`/settings/agents/new?parent=${agent.id}`)}
                  >
                    {agent.display_name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>
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
        <div className="space-y-3">
          {displayedAgents.map(({ agent, isSubagent }) => (
            <div
              key={agent.id}
              className={isSubagent ? "ml-6 pl-4 border-l-2 border-muted" : ""}
            >
              <AgentCard
                agent={agent}
                providers={providerList}
                onEdit={handleEdit}
                onDelete={(id) => setDeleteId(id)}
              />
            </div>
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
