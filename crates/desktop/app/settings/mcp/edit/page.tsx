import { useMemo } from "react"
import { useSearchParams } from "react-router-dom"
import { McpEditor } from "@/components/settings"

export default function EditMcpPage() {
  const [searchParams] = useSearchParams();
  const serverId = useMemo(() => {
    const id = searchParams.get("id");
    return id ? parseInt(id, 10) : undefined;
  }, [searchParams]);
  return <McpEditor serverId={serverId} />;
}
