import { ProviderEditor } from "@/components/settings";

export default async function EditProviderPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return <ProviderEditor providerId={parseInt(id)} />;
}
