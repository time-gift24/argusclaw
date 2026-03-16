"use client";

import * as React from "react";
import {
  providers,
  type LlmProviderSummary,
  type ProviderInput,
  type ProviderTestResult,
} from "@/lib/tauri";
import {
  ProviderCard,
  ProviderFormDialog,
  ProviderTestDialog,
  type LlmProviderRecord,
  DeleteConfirmDialog,
  Breadcrumb,
} from "@/components/settings";

export default function ProvidersPage() {
  const [providerList, setProviderList] = React.useState<LlmProviderSummary[]>(
    [],
  );
  const [loading, setLoading] = React.useState(true);
  const [editingProvider, setEditingProvider] =
    React.useState<LlmProviderRecord | null>(null);
  const [deleteId, setDeleteId] = React.useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = React.useState(false);
  const [testResultsByProviderId, setTestResultsByProviderId] = React.useState<
    Record<string, ProviderTestResult>
  >({});
  const [activeProviderId, setActiveProviderId] = React.useState<string | null>(
    null,
  );
  const [testDialogOpen, setTestDialogOpen] = React.useState(false);
  const [testingProviderId, setTestingProviderId] = React.useState<
    string | null
  >(null);

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

  const handleSubmit = async (record: LlmProviderRecord) => {
    const input: ProviderInput = {
      id: record.id,
      kind: record.kind,
      display_name: record.display_name,
      base_url: record.base_url,
      api_key: record.api_key,
      is_default: record.is_default,
      extra_headers: record.extra_headers,
    };
    await providers.upsert(input);
    setEditingProvider(null);
    await loadProviders();
  };

  const handleEdit = async (id: string) => {
    const provider = await providers.get(id);
    if (provider) {
      // Transform from API format to form format
      const formRecord: LlmProviderRecord = {
        id: provider.id,
        kind: provider.kind,
        display_name: provider.display_name,
        base_url: provider.base_url,
        api_key:
          typeof provider.api_key === "string"
            ? provider.api_key
            : (provider.api_key as { api_key: string }).api_key || "",
        is_default: provider.is_default,
        extra_headers: provider.extra_headers,
        secret_status: provider.secret_status,
      };
      setEditingProvider(formRecord);
    }
  };

  const handleDelete = async () => {
    if (!deleteId) return;
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

  const handleSetDefault = async (id: string) => {
    await providers.setDefault(id);
    await loadProviders();
  };

  const runConnectionTest = React.useCallback(
    async (id: string) => {
      const provider = providerList.find((item) => item.id === id);
      setTestingProviderId(id);
      try {
        const result = await providers.testConnection(id);
        setTestResultsByProviderId((current) => ({ ...current, [id]: result }));
      } catch (error) {
        const fallbackResult: ProviderTestResult = {
          provider_id: id,
          model: "",
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
    (id: string) => {
      const provider = providerList.find((item) => item.id === id);
      if (provider?.secret_status === "requires_reentry") {
        return;
      }
      setActiveProviderId(id);
      setTestDialogOpen(true);
      void runConnectionTest(id);
    },
    [providerList, runConnectionTest],
  );

  const handleViewStatus = React.useCallback((id: string) => {
    setActiveProviderId(id);
    setTestDialogOpen(true);
  }, []);

  const handleRetest = React.useCallback(() => {
    if (!activeProviderId) return;
    void runConnectionTest(activeProviderId);
  }, [activeProviderId, runConnectionTest]);

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
    <div className="mx-auto w-full max-w-7xl px-6 py-6 space-y-4">
      <Breadcrumb
        items={[{ label: "设置", href: "/settings" }, { label: "LLM 提供者" }]}
      />

      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-sm font-semibold">LLM 提供者</h1>
          <p className="text-muted-foreground text-xs">
            配置你的 LLM 提供者连接
          </p>
        </div>
        <ProviderFormDialog onSubmit={handleSubmit} />
      </div>

      {providerList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground mb-4">No providers configured</p>
          <ProviderFormDialog onSubmit={handleSubmit} />
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {providerList.map((provider) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              onEdit={handleEdit}
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

      {/* Edit Dialog */}
      {editingProvider && (
        <ProviderFormDialog
          provider={editingProvider}
          onSubmit={handleSubmit}
          open={!!editingProvider}
          onOpenChange={(open) => !open && setEditingProvider(null)}
          trigger={null}
        />
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
          }
        }}
        provider={activeProvider}
        result={activeTestResult}
        testing={
          activeProviderId !== null && testingProviderId === activeProviderId
        }
        onRetest={handleRetest}
      />
    </div>
  );
}
