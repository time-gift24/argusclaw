import { AgentEditor } from "@/components/settings"

export default async function EditAgentPage({
  params,
}: {
  params: Promise<{ id: string }>
}) {
  const { id } = await params

  return <AgentEditor agentId={parseInt(id)} />
}
