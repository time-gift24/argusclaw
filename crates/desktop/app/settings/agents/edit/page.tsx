'use client'

import { useSearchParams } from "next/navigation"
import { useMemo, Suspense } from "react"
import { AgentEditor } from "@/components/settings"

function EditAgentContent() {
  const searchParams = useSearchParams()
  const agentId = useMemo(() => {
    const id = searchParams.get("id")
    return id ? parseInt(id) : undefined
  }, [searchParams])

  return <AgentEditor agentId={agentId} />
}

export default function EditAgentPage() {
  return (
    <Suspense fallback={<div className="flex items-center justify-center h-64"><div className="text-muted-foreground">加载中...</div></div>}>
      <EditAgentContent />
    </Suspense>
  )
}
