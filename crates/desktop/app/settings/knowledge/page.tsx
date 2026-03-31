"use client"

import * as React from "react"
import { BookOpen, Plus, Trash2, Github, Tag } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import {
  knowledge,
  type KnowledgeRepoRecord,
} from "@/lib/tauri"
import { DeleteConfirmDialog } from "@/components/settings"

export default function KnowledgePage() {
  const [repoList, setRepoList] = React.useState<KnowledgeRepoRecord[]>([])
  const [loading, setLoading] = React.useState(true)
  const [deleteId, setDeleteId] = React.useState<number | null>(null)
  const [deleteLoading, setDeleteLoading] = React.useState(false)

  // Add form state
  const [showAddForm, setShowAddForm] = React.useState(false)
  const [addRepo, setAddRepo] = React.useState("")
  const [addWorkspace, setAddWorkspace] = React.useState("")
  const [addLoading, setAddLoading] = React.useState(false)

  const loadData = React.useCallback(async () => {
    try {
      const data = await knowledge.list()
      setRepoList(data)
    } catch (error) {
      console.error("Failed to load knowledge repos:", error)
    } finally {
      setLoading(false)
    }
  }, [])

  React.useEffect(() => {
    loadData()
  }, [loadData])

  const handleAdd = async () => {
    if (!addRepo.trim() || !addWorkspace.trim()) return
    setAddLoading(true)
    try {
      const parts = addRepo.trim().split("/")
      const owner = parts[0] || ""
      const name = parts.slice(1).join("/") || ""
      await knowledge.upsert({
        id: 0,
        repo: addRepo.trim(),
        repo_id: addRepo.trim().toLowerCase(),
        provider: "github",
        owner,
        name,
        default_branch: "main",
        manifest_paths: [],
        workspace: addWorkspace.trim(),
      })
      setAddRepo("")
      setAddWorkspace("")
      setShowAddForm(false)
      await loadData()
    } catch (error) {
      console.error("Failed to add repo:", error)
    } finally {
      setAddLoading(false)
    }
  }

  const handleDelete = async () => {
    if (!deleteId) return
    setDeleteLoading(true)
    try {
      await knowledge.delete(deleteId)
      setDeleteId(null)
      await loadData()
    } finally {
      setDeleteLoading(false)
    }
  }

  // Group repos by workspace
  const workspaces = React.useMemo(() => {
    const map = new Map<string, KnowledgeRepoRecord[]>()
    for (const repo of repoList) {
      const ws = repo.workspace || "default"
      if (!map.has(ws)) map.set(ws, [])
      map.get(ws)!.push(repo)
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b))
  }, [repoList])

  // Extract unique workspace names for suggestion
  const workspaceNames = React.useMemo(
    () => Array.from(new Set(repoList.map((r) => r.workspace).filter(Boolean))),
    [repoList],
  )

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载知识仓库...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-6 animate-in fade-in duration-500">
      {/* Header */}
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between border-b pb-6">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <BookOpen className="h-5 w-5 text-primary" />
            <h1 className="text-xl font-bold tracking-tight">知识仓库配置</h1>
          </div>
          <p className="text-muted-foreground text-sm">
            管理知识仓库，按工作区分组绑定到智能体。
          </p>
        </div>
        <Button size="sm" onClick={() => setShowAddForm(!showAddForm)} className="h-9 shadow-sm">
          <Plus className="h-4 w-4 mr-1.5" />
          添加仓库
        </Button>
      </div>

      {/* Add form */}
      {showAddForm && (
        <div className="bg-muted/20 p-6 rounded-3xl border border-muted/60 shadow-sm space-y-4">
          <div className="text-[11px] font-bold text-primary uppercase tracking-widest px-1">
            添加新仓库
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">
                仓库地址 (owner/name)
              </Label>
              <div className="relative">
                <Github className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                <Input
                  value={addRepo}
                  onChange={(e) => setAddRepo(e.target.value)}
                  placeholder="例如: rust-lang/rust"
                  className="h-10 pl-9 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm"
                />
              </div>
            </div>
            <div className="space-y-2">
              <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">
                工作区 (workspace)
              </Label>
              <div className="relative">
                <Tag className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                <Input
                  value={addWorkspace}
                  onChange={(e) => setAddWorkspace(e.target.value)}
                  placeholder="例如: rust"
                  className="h-10 pl-9 bg-background border-muted/60 focus-visible:ring-primary/20 text-sm"
                  list="workspace-suggestions"
                />
              </div>
              {workspaceNames.length > 0 && (
                <datalist id="workspace-suggestions">
                  {workspaceNames.map((ws) => (
                    <option key={ws} value={ws} />
                  ))}
                </datalist>
              )}
            </div>
            <div className="flex items-end gap-2">
              <Button
                size="sm"
                onClick={handleAdd}
                disabled={addLoading || !addRepo.trim() || !addWorkspace.trim()}
                className="h-10 px-6"
              >
                {addLoading ? "添加中..." : "确认添加"}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => { setShowAddForm(false); setAddRepo(""); setAddWorkspace("") }}
                className="h-10"
              >
                取消
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Content */}
      {repoList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-80 border-2 border-dashed rounded-2xl bg-muted/20 gap-4">
          <div className="bg-muted p-4 rounded-full">
            <BookOpen className="h-8 w-8 text-muted-foreground/50" />
          </div>
          <div className="text-center space-y-1">
            <p className="font-medium text-muted-foreground">暂无知识仓库</p>
            <p className="text-xs text-muted-foreground/60">添加 GitHub 仓库作为智能体的知识来源</p>
          </div>
          <Button size="sm" onClick={() => setShowAddForm(true)} className="px-6">
            <Plus className="h-4 w-4 mr-1.5" />
            立即添加
          </Button>
        </div>
      ) : (
        <div className="space-y-8">
          {workspaces.map(([workspace, repos]) => (
            <div key={workspace} className="space-y-3">
              <div className="flex items-center gap-2">
                <Badge variant="secondary" className="text-xs font-mono">
                  {workspace}
                </Badge>
                <span className="text-xs text-muted-foreground">
                  {repos.length} 个仓库
                </span>
              </div>
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                {repos.map((repo) => (
                  <div
                    key={repo.id}
                    className="group flex items-center gap-4 rounded-2xl border border-muted/60 bg-background p-4 transition-all hover:border-primary/30"
                  >
                    <div className="bg-muted/50 p-2 rounded-lg shrink-0">
                      <Github className="h-5 w-5 text-muted-foreground" />
                    </div>
                    <div className="flex-1 min-w-0 space-y-0.5">
                      <p className="text-sm font-semibold truncate">
                        {repo.owner}/{repo.name}
                      </p>
                      <p className="text-[10px] text-muted-foreground font-mono">
                        {repo.default_branch} {repo.provider}
                      </p>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-destructive"
                      onClick={() => setDeleteId(repo.id)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Delete confirmation */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="确认删除仓库"
        description="此操作将从知识库中移除该仓库，且无法撤销。确定要继续吗？"
        onConfirm={handleDelete}
        loading={deleteLoading}
      />
    </div>
  )
}
