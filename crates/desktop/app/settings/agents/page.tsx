"use client"

import * as React from "react"
import { Link, useNavigate } from "react-router-dom"
import { Plus, ChevronDown, Bot, Layers } from "lucide-react"
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
import { Badge } from "@/components/ui/badge"

export default function AgentsPage() {
  const navigate = useNavigate()
  const [agentList, setAgentList] = React.useState<AgentRecord[]>([])
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [loading, setLoading] = React.useState(true)
  const [showSubagents, setShowSubagents] = React.useState(true)
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

  // Separate parent agents (standard agents without parent) from subagents
  const parentAgents = agentList.filter(
    (agent) => !agent.parent_agent_id && agent.agent_type !== "subagent"
  )
  const subagents = agentList.filter(
    (agent) => agent.parent_agent_id || agent.agent_type === "subagent"
  )

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载智能体...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-6 animate-in fade-in duration-500">
      {/* 顶部标题栏 */}
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between border-b pb-6">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Bot className="h-5 w-5 text-primary" />
            <h1 className="text-xl font-bold tracking-tight">智能体管理</h1>
          </div>
          <p className="text-muted-foreground text-sm">
            在这里创建、配置和管理您的 AI 智能体及其子智能体。
          </p>
        </div>

        <div className="flex items-center gap-2">
          {subagents.length > 0 && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowSubagents(!showSubagents)}
              className="h-9"
            >
              <Layers className={`h-4 w-4 mr-2 transition-colors ${showSubagents ? 'text-primary' : ''}`} />
              {showSubagents ? "隐藏子智能体" : "显示子智能体"}
              <Badge variant="secondary" className="ml-2 px-1 py-0 h-4 min-w-4 flex items-center justify-center">
                {subagents.length}
              </Badge>
            </Button>
          )}

          <div className="flex items-center gap-2">
            <Link to="/settings/agents/new">
              <Button size="sm" className="h-9 shadow-sm">
                <Plus className="h-4 w-4 mr-1.5" />
                新建智能体
              </Button>
            </Link>

            {parentAgents.length > 0 && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button size="sm" variant="secondary" className="h-9">
                    <Plus className="h-4 w-4 mr-1.5" />
                    新建子智能体
                    <ChevronDown className="h-3.3 w-3.3 ml-1.5 opacity-50" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-48">
                  <div className="px-2 py-1.5 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                    选择父智能体
                  </div>
                  {parentAgents.map((agent) => (
                    <DropdownMenuItem
                      key={agent.id}
                      onClick={() => navigate(`/settings/agents/new?parent=${agent.id}`)}
                      className="cursor-pointer"
                    >
                      {agent.display_name}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </div>
        </div>
      </div>

      {/* 智能体列表 */}
      {agentList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-80 border-2 border-dashed rounded-2xl bg-muted/20 gap-4">
          <div className="bg-muted p-4 rounded-full">
            <Bot className="h-8 w-8 text-muted-foreground/50" />
          </div>
          <div className="text-center space-y-1">
            <p className="font-medium text-muted-foreground">暂无智能体配置</p>
            <p className="text-xs text-muted-foreground/60">开始创建您的第一个 AI 助手吧</p>
          </div>
          <Link to="/settings/agents/new">
            <Button size="sm" className="px-6">
              <Plus className="h-4 w-4 mr-1.5" />
              立即创建
            </Button>
          </Link>
        </div>
      ) : (
        <div className="grid gap-6">
          {parentAgents.map((parent) => (
            <div key={parent.id} className="space-y-4">
              <AgentCard
                agent={parent}
                providers={providerList}
                onEdit={handleEdit}
                onDelete={(id) => setDeleteId(id)}
              />

              {showSubagents && (
                <div className="grid gap-4 ml-6 pl-6 border-l-2 border-primary/10 relative">
                  {subagents
                    .filter((sub) => sub.parent_agent_id === parent.id)
                    .map((sub) => (
                      <div key={sub.id} className="relative">
                        <div className="absolute -left-[26px] top-1/2 w-4 h-[2px] bg-primary/10" />
                        <AgentCard
                          agent={sub}
                          providers={providerList}
                          onEdit={handleEdit}
                          onDelete={(id) => setDeleteId(id)}
                        />
                      </div>
                    ))}
                </div>
              )}
            </div>
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
