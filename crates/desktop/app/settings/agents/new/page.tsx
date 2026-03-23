"use client"

import * as React from "react"
import { useSearchParams } from "next/navigation"
import { AgentEditor } from "@/components/settings"

function NewAgentContent() {
  const searchParams = useSearchParams()
  const parentId = searchParams.get("parent")
  return <AgentEditor parentId={parentId ? parseInt(parentId) : undefined} />
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
