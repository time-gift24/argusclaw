import { AgentEditor } from "@/components/settings"

export default function EditAgentPage({ params }: { params: { id: string } }) {
  return <AgentEditor agentId={params.id} />
}
