"use client"

import * as React from "react"
import { agents, providers, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import {
  AgentCard,
  AgentFormDialog,
  DeleteConfirmDialog,
} from "@/components/settings"

export default function AgentsPage() {
  const [agentList, setAgentList] = React.useState<AgentRecord[]>([])
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>([])
  const [loading, setLoading] = React.useState(true)
  const [editingAgent, setEditingAgent] = React.useState<AgentRecord | null>(null)
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

  const handleCreate = async (record: AgentRecord) => {
    await agents.upsert(record)
    await loadData()
  }

  const handleEdit = async (id: string) => {
    const agent = await agents.get(id)
    if (agent) {
      setEditingAgent(agent)
    }
  }

  const handleUpdate = async (record: AgentRecord) => {
    await agents.upsert(record)
    setEditingAgent(null)
    await loadData()
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
        <div className="text-muted-foreground">Loading agents...</div>
      </div>
    )
  }

  return (
    <div className="mx-auto max-w-7xl px-6 py-8 space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-sm font-semibold">智能体</h1>
          <p className="text-muted-foreground text-xs">
            配置你的 AI 智能体
          </p>
        </div>
        <AgentFormDialog providers={providerList} onSubmit={handleCreate} />
      </div>

      {providerList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-32 border rounded-lg border-dashed">
          <p className="text-muted-foreground text-sm">
            Please configure a provider first
          </p>
        </div>
      ) : agentList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground mb-4">No agents configured</p>
          <AgentFormDialog providers={providerList} onSubmit={handleCreate} />
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {agentList.map((agent) => (
            <AgentCard
              key={agent.id}
              agent={agent}
              onEdit={handleEdit}
              onDelete={(id) => setDeleteId(id)}
            />
          ))}
        </div>
      )}

      {/* Edit Dialog */}
      {editingAgent && (
        <AgentFormDialog
          agent={editingAgent}
          providers={providerList}
          onSubmit={handleUpdate}
          trigger={<span />}
        />
      )}

      {/* Delete Confirmation */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="Delete Agent"
        description="Are you sure you want to delete this agent? This action cannot be undone."
        onConfirm={handleDelete}
        loading={deleteLoading}
      />
    </div>
  )
}
