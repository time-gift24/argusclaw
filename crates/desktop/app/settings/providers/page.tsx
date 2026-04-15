"use client";

import * as React from "react";
import { useNavigate } from "react-router-dom";
import { Plus, Cloud } from "lucide-react";
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
  const navigate = useNavigate();
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
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载提供者...</div>
      </div>
    );
  }

  return (
    <div className="w-full space-y-4 animate-in fade-in duration-500">
      {/* 顶部标题栏 */}
      <div className="flex flex-col gap-3 border-b pb-4 md:flex-row md:items-center md:justify-between">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Cloud className="h-4 w-4 text-primary" />
            <h1 className="text-lg font-bold tracking-tight">模型提供者 (LLM Providers)</h1>
          </div>
          <p className="text-muted-foreground text-sm">
            管理您的 AI 模型提供者连接、API 密钥及模型列表。
          </p>
        </div>

        <Button size="sm" onClick={() => navigate("/settings/providers/new")} className="shadow-sm">
          <Plus className="h-4 w-4 mr-1.5" />
          添加提供者
        </Button>
      </div>

      {providerList.length === 0 ? (
        <div className="flex h-64 flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed bg-muted/20">
          <div className="rounded-full bg-muted p-3">
            <Cloud className="h-7 w-7 text-muted-foreground/50" />
          </div>
          <div className="text-center space-y-1">
            <p className="font-medium text-muted-foreground">暂无模型提供者</p>
            <p className="text-xs text-muted-foreground/60">开始配置您的第一个 AI 推理服务吧</p>
          </div>
          <Button size="sm" onClick={() => navigate("/settings/providers/new")} className="px-4">
            <Plus className="h-4 w-4 mr-1.5" />
            立即添加
          </Button>
        </div>
      ) : (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
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

      {/* 删除确认对话框 */}
      <DeleteConfirmDialog
        open={!!deleteId}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title="确认删除提供者"
        description="此操作将永久删除该模型提供者配置，且无法撤销。您确定要继续吗？"
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
