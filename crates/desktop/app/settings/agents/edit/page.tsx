'use client'

import { useMemo } from "react"
import { useSearchParams } from "react-router-dom"
import { AgentEditor } from "@/components/settings"

export default function EditAgentPage() {
  const searchParams = useSearchParams()
  const agentId = useMemo(() => {
    const id = searchParams.get("id")
    return id ? parseInt(id, 10) : undefined
  }, [searchParams])

  return <AgentEditor agentId={agentId} />
}
