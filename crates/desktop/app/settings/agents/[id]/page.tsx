import { AgentEditor } from "@/components/settings"

function safeDecodeRouteId(value: string) {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

export default async function EditAgentPage({
  params,
}: {
  params: Promise<{ id: string }>
}) {
  const { id } = await params
  const agentId = safeDecodeRouteId(id)

  return <AgentEditor agentId={agentId} />
}
