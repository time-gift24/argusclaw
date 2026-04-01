'use client'

import { Suspense, useMemo } from "react"
import { useSearchParams } from "next/navigation"

import { McpEditor } from "@/components/settings"

function EditMcpContent() {
  const searchParams = useSearchParams()
  const serverId = useMemo(() => {
    const id = searchParams.get("id")
    return id ? parseInt(id, 10) : undefined
  }, [searchParams])

  return <McpEditor serverId={serverId} />
}

export default function EditMcpPage() {
  return (
    <Suspense
      fallback={
        <div className="flex items-center justify-center h-64">
          <div className="text-muted-foreground">加载中...</div>
        </div>
      }
    >
      <EditMcpContent />
    </Suspense>
  )
}
