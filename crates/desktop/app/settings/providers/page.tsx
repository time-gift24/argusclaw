"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  providers,
  type LlmProviderSummary,
  type ProviderTestResult,
} from "@/lib/tauri";
import {
  ProviderCard,
  ProviderTestDialog,
  DeleteConfirmDialog,
} from "@/components/settings";

export default function ProvidersPage() {
  const router = useRouter();
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>(
    [],
  );
  const [loading, setLoading] = React.useState(true);
  const [deleteId, setDeleteId] = React.useState<number | null>(null);
  const [deleteLoading, setDeleteLoading] = React.useState(false);
  const [testResultsByProviderId, setTestResultsByProviderId] = React.useState<
    Record<number, ProviderTestResult>
  >({});
  const [activeProviderId, setActiveProviderId] = React.useState<number | null>(
    null,
  );
  const [testDialogOpen, setTestDialogOpen] = React.useState(false);
  const [testingProviderId, setTestingProviderId] = React.useState<
    number | null
  >(null);
  const [testSelectedModel, setTestSelectedModel] = React.useState<string | null>(null);

  const loadProviders = React.useCallback(async () => {
    try {
      const data = await providers.list();
      setProviderList(data);
    } catch (error) {
      console.error("Failed to load providers:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  React.useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  const handleDelete = async () => {
    if (deleteId === null) return;
    setDeleteLoading(true);
    try {
      await providers.delete(deleteId);
      setTestResultsByProviderId((current) => {
        const next = { ...current };
        delete next[deleteId];
        return next;
      });
      if (activeProviderId === deleteId) {
        setActiveProviderId(null);
        setTestDialogOpen(false);
      }
      setDeleteId(null);
      await loadProviders();
    } finally {
      setDeleteLoading(false);
    }
  };

  const handleSetDefault = async (id: number) => {
    await providers.setDefault(id);
    await loadProviders();
  };

  const runConnectionTest = React.useCallback(
    async (id: number, model: string) => {
      const provider = providerList.find((item) => item.id === id);
      setTestingProviderId(id);
      try {
        const result = await providers.testConnection(id, model);
        setTestResultsByProviderId((current) => ({ ...current, [id]: result }));
      } catch (error) {
        const fallbackResult: ProviderTestResult = {
          provider_id: String(id),
          model,
          base_url: provider?.base_url ?? "",
          checked_at: new Date().toISOString(),
          latency_ms: 0,
          status: "request_failed",
          message: error instanceof Error ? error.message : String(error),
        };
        setTestResultsByProviderId((current) => ({
          ...current,
          [id]: fallbackResult,
        }));
        console.error("Failed to test provider connection:", error);
      } finally {
        setTestingProviderId((current) => (current === id ? null : current));
      }
    },
    [providerList],
  );

  const handleTestConnection = React.useCallback(
    (id: number) => {
      const provider = providerList.find((item) => item.id === id);
      if (provider?.secret_status === "requires_reentry") {
        return;
      }
      setActiveProviderId(id);
      setTestSelectedModel(provider?.default_model || null);
      setTestDialogOpen(true);
      void runConnectionTest(id, provider?.default_model || "");
    },
    [providerList, runConnectionTest],
  );

  const handleViewStatus = React.useCallback((id: number) => {
    const provider = providerList.find((item) => item.id === id);
    setActiveProviderId(id);
    setTestSelectedModel(provider?.default_model || null);
    setTestDialogOpen(true);
  }, [providerList]);

  const handleRetest = React.useCallback(() => {
    if (!activeProviderId || !testSelectedModel) return;
    void runConnectionTest(activeProviderId, testSelectedModel);
  }, [activeProviderId, testSelectedModel, runConnectionTest]);

  const activeProvider = React.useMemo(
    () =>
      providerList.find((provider) => provider.id === activeProviderId) ?? null,
    [activeProviderId, providerList],
  );
  const activeTestResult = activeProviderId
    ? (testResultsByProviderId[activeProviderId] ?? null)
    : null;

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">Loading providers...</div>
      </div>
    );
  }

  return (
    <div className="w-full space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-sm font-semibold">LLM 提供者</h1>
          <p className="text-muted-foreground text-xs">
            配置你的 LLM 提供者连接
          </p>
        </div>
        <Button size="sm" onClick={() => router.push("/settings/providers/new")}>
          <Plus className="h-4 w-4 mr-1" />
          Add Provider
        </Button>
      </div>

      {providerList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground mb-4">No providers configured</p>
          <Button size="sm" onClick={() => router.push("/settings/providers/new")}>
            <Plus className="h-4 w-4 mr-1" />
            Add Provider
          </Button>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {providerList.map((provider) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              onDelete={(id) => setDeleteId(id)}
              onSetDefault={handleSetDefault}
              onTestConnection={handleTestConnection}
              onViewStatus={handleViewStatus}
              testResult={testResultsByProviderId[provider.id]}
              isTesting={testingProviderId === provider.id}
            />
          ))}
        </div>
      )}

      {/* Delete Confirmation */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="Delete Provider"
        description="Are you sure you want to delete this provider? This action cannot be undone."
        onConfirm={handleDelete}
        loading={deleteLoading}
      />

      <ProviderTestDialog
        open={testDialogOpen}
        onOpenChange={(open) => {
          setTestDialogOpen(open);
          if (!open) {
            setActiveProviderId(null);
            setTestSelectedModel(null);
          }
        }}
        provider={activeProvider}
        result={activeTestResult}
        selectedModel={testSelectedModel || undefined}
        onModelChange={(model) => setTestSelectedModel(model)}
        testing={
          activeProviderId !== null && testingProviderId === activeProviderId
        }
        onRetest={handleRetest}
      />
    </div>
  );
}
