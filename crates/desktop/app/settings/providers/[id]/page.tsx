import { ProviderEditor } from "@/components/settings";

function safeDecodeRouteId(value: string) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

export default async function EditProviderPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const providerId = safeDecodeRouteId(id);

  return <ProviderEditor providerId={providerId} />;
}
