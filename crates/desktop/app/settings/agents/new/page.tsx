"use client"

import { useSearchParams } from "react-router-dom"
import { AgentEditor } from "@/components/settings"

export default function NewAgentPage() {
  const searchParams = useSearchParams()
  const rawParentId = searchParams.get("parent")
  const parsed = rawParentId ? parseInt(rawParentId, 10) : undefined
  const parentId = Number.isFinite(parsed) ? parsed : undefined
  return <AgentEditor parentId={parentId} />
}
