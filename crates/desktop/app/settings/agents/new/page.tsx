"use client"

import * as React from "react"
import { useSearchParams } from "next/navigation"
import { AgentEditor } from "@/components/settings"

function NewAgentContent() {
  const searchParams = useSearchParams()
  const rawParentId = searchParams.get("parent")
  const parsed = rawParentId ? parseInt(rawParentId, 10) : undefined
  const parentId = Number.isFinite(parsed) ? parsed : undefined
  return <AgentEditor parentId={parentId} />
}

export default function NewAgentPage() {
  return (
    <React.Suspense fallback={
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    }>
      <NewAgentContent />
    </React.Suspense>
  )
}
