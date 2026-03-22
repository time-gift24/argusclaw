import { McpServerEditor } from "@/components/settings";

export default async function EditMcpServerPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return <McpServerEditor serverId={parseInt(id)} />;
}
