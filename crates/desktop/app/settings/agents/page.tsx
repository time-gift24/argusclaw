"use client"

import * as React from "react"
import Link from "next/link"
import { useRouter } from "next/navigation"
import { Plus, UserPlus, X } from "lucide-react"
import { agents, providers, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import {
  AgentCard,
  DeleteConfirmDialog,
} from "@/components/settings"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

export default function AgentsPage() {
  const router = useRouter()
  const [agentList, setAgentList] = React.useState<AgentRecord[]>([])
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [loading, setLoading] = React.useState(true)
  const [deleteId, setDeleteId] = React.useState<number | null>(null)
  const [deleteLoading, setDeleteLoading] = React.useState(false)
  const [addSubagentOpen, setAddSubagentOpen] = React.useState(false)
  const [addSubagentParentId, setAddSubagentParentId] = React.useState<number | null>(null)
  const [addSubagentChildId, setAddSubagentChildId] = React.useState<number | null>(null)
  const [addSubagentLoading, setAddSubagentLoading] = React.useState(false)

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

  const handleAddSubagent = async () => {
    if (!addSubagentParentId || !addSubagentChildId) return
    setAddSubagentLoading(true)
    try {
      await agents.addSubagent(addSubagentParentId, addSubagentChildId)
      setAddSubagentOpen(false)
      setAddSubagentParentId(null)
      setAddSubagentChildId(null)
      await loadData()
    } finally {
      setAddSubagentLoading(false)
    }
  }

  const handleRemoveSubagent = async (parentId: number, childId: number) => {
    try {
      await agents.removeSubagent(parentId, childId)
      await loadData()
    } catch (error) {
      console.error("Failed to remove subagent:", error)
    }
  }

  const openAddSubagent = (parentId: number) => {
    setAddSubagentParentId(parentId)
    setAddSubagentChildId(null)
    setAddSubagentOpen(true)
  }

  // Separate parent agents (standard agents without parent) from subagents
  const parentAgents = agentList.filter(
    (agent) => !agent.parent_agent_id && agent.agent_type !== "subagent"
  )
  const subagents = agentList.filter(
    (agent) => agent.parent_agent_id || agent.agent_type === "subagent"
  )

  // Get available agents that can be added as subagents (agents that are not already subagents and are not the parent)
  const availableSubagentCandidates = agentList.filter(
    (agent) =>
      !agent.parent_agent_id &&
      agent.agent_type !== "subagent" &&
      agent.id !== addSubagentParentId
  )

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
        <div className="space-y-6">
          {parentAgents.map((agent) => {
            const agentSubagents = subagents.filter((s) => s.parent_agent_id === agent.id)
            return (
              <div key={agent.id} className="space-y-3">
                <div className="flex items-start gap-3">
                  <div className="flex-1">
                    <AgentCard
                      agent={agent}
                      providers={providerList}
                      onEdit={handleEdit}
                      onDelete={(id) => setDeleteId(id)}
                    />
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => openAddSubagent(agent.id)}
                    className="mt-1"
                  >
                    <UserPlus className="h-3 w-3 mr-1" />
                    添加子智能体
                  </Button>
                </div>

                {/* Subagent list */}
                {agentSubagents.length > 0 && (
                  <div className="ml-6 space-y-2 border-l-2 border-muted pl-4">
                    <p className="text-xs text-muted-foreground font-medium">
                      子智能体 ({agentSubagents.length})
                    </p>
                    <div className="space-y-2">
                      {agentSubagents.map((subagent) => (
                        <div
                          key={subagent.id}
                          className="flex items-center gap-2 p-2 rounded-md border bg-muted/20"
                        >
                          <div className="flex-1 min-w-0">
                            <p className="text-sm font-medium truncate">
                              {subagent.display_name}
                            </p>
                            <p className="text-xs text-muted-foreground">
                              v{subagent.version} · ID: {subagent.id}
                            </p>
                          </div>
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => handleRemoveSubagent(agent.id, subagent.id)}
                          >
                            <X className="h-3 w-3" />
                          </Button>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )
          })}
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

      {/* Add Subagent Dialog */}
      <Dialog open={addSubagentOpen} onOpenChange={setAddSubagentOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>添加子智能体</DialogTitle>
            <DialogDescription>
              选择一个智能体作为当前智能体的子智能体
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <label className="text-sm font-medium">选择子智能体</label>
              <Select
                value={addSubagentChildId?.toString() ?? ""}
                onValueChange={(value) => setAddSubagentChildId(value ? parseInt(value, 10) : null)}
              >
                <SelectTrigger>
                  <SelectValue placeholder="选择智能体" />
                </SelectTrigger>
                <SelectContent>
                  {availableSubagentCandidates.map((candidate) => (
                    <SelectItem key={candidate.id} value={candidate.id.toString()}>
                      {candidate.display_name} (v{candidate.version})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddSubagentOpen(false)}>
              取消
            </Button>
            <Button
              onClick={handleAddSubagent}
              disabled={!addSubagentChildId || addSubagentLoading}
            >
              {addSubagentLoading ? "添加中..." : "添加"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
