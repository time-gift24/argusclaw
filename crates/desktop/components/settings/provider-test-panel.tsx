"use client";

import * as React from "react";
import {
  CircleAlert,
  CircleCheckBig,
  LoaderCircle,
  RefreshCw,
} from "lucide-react";
import type {
  ProviderSecretStatus,
  ProviderTestResult,
  ProviderTestStatus,
} from "@/lib/tauri";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export type ModelTestState = {
  model: string;
  status: "idle" | "testing" | "success" | "error";
  result?: ProviderTestResult;
};

interface ProviderTestPanelProps {
  models: string[];
  defaultModel: string;
  providerId?: number;
  getTestInput: () => {
    id: number;
    kind: "openai-compatible";
    display_name: string;
    base_url: string;
    api_key: string;
    models: string[];
    default_model: string;
    is_default: boolean;
    extra_headers: Record<string, string>;
    secret_status: ProviderSecretStatus;
  } | null;
  canTest: boolean;
}

const statusLabels: Record<ProviderTestStatus, string> = {
  success: "成功",
  auth_failed: "认证失败",
  model_not_available: "模型不可用",
  rate_limited: "请求限制",
  request_failed: "请求失败",
  invalid_response: "响应无效",
  provider_not_found: "Provider 未找到",
  unsupported_provider_kind: "不支持的类型",
};

export function ProviderTestPanel({
  models,
  defaultModel,
  providerId,
  getTestInput,
  canTest,
}: ProviderTestPanelProps) {
  const [testStates, setTestStates] = React.useState<Record<string, ModelTestState>>({});
  const [testingAll, setTestingAll] = React.useState(false);
  const [expandedError, setExpandedError] = React.useState<string | null>(null);

  const runTest = React.useCallback(
    async (model: string) => {
      const input = getTestInput();
      if (!input) return;

      setTestStates((prev) => ({
        ...prev,
        [model]: { model, status: "testing" },
      }));

      try {
        const { providers } = await import("@/lib/tauri");
        const result = providerId
          ? await providers.testConnection(providerId, model)
          : await providers.testInput(input, model);

        setTestStates((prev) => ({
          ...prev,
          [model]: { model, status: result.status === "success" ? "success" : "error", result },
        }));
      } catch (error) {
        setTestStates((prev) => ({
          ...prev,
          [model]: {
            model,
            status: "error",
            result: {
              provider_id: String(input.id),
              model,
              base_url: input.base_url,
              checked_at: new Date().toISOString(),
              latency_ms: 0,
              status: "request_failed",
              message: error instanceof Error ? error.message : String(error),
            },
          },
        }));
      }
    },
    [getTestInput, providerId]
  );

  const testAll = React.useCallback(async () => {
    if (!canTest || models.length === 0) return;
    setTestingAll(true);
    for (const model of models) {
      await runTest(model);
    }
    setTestingAll(false);
  }, [canTest, models, runTest]);

  // Auto-test when models change
  React.useEffect(() => {
    if (!canTest) return;
    const newModels = models.filter((m) => !testStates[m]);
    if (newModels.length > 0) {
      void runTest(newModels[0]);
    }
  }, [models, canTest, runTest, testStates]);

  const successCount = Object.values(testStates).filter(
    (s) => s.status === "success"
  ).length;
  const avgLatency =
    successCount > 0
      ? Math.round(
          Object.values(testStates)
            .filter((s) => s.status === "success" && s.result)
            .reduce((sum, s) => sum + (s.result?.latency_ms || 0), 0) / successCount
        )
      : null;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground font-medium uppercase">
          模型可达性测试
        </span>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void testAll()}
          disabled={!canTest || testingAll || models.length === 0}
        >
          {testingAll ? (
            <LoaderCircle className="h-3 w-3 mr-1 animate-spin" />
          ) : (
            <RefreshCw className="h-3 w-3 mr-1" />
          )}
          全部测试
        </Button>
      </div>

      {models.length === 0 ? (
        <div className="text-sm text-muted-foreground py-8 text-center border rounded-lg border-dashed">
          添加模型后可进行可达性测试
        </div>
      ) : (
        <div className="border rounded-lg overflow-hidden">
          {models.map((model) => {
            const state = testStates[model];
            const isDefault = model === defaultModel;

            return (
              <div key={model}>
                <div
                  className={`px-3 py-2 flex items-center justify-between cursor-pointer hover:bg-muted/50 ${
                    state?.status === "error" ? "bg-red-50" : ""
                  } ${state?.status === "testing" ? "bg-blue-50" : ""}`}
                  onClick={() => state?.status !== "testing" && void runTest(model)}
                >
                  <div className="flex items-center gap-2">
                    {state?.status === "testing" ? (
                      <LoaderCircle className="h-4 w-4 text-blue-500 animate-spin" />
                    ) : state?.status === "success" ? (
                      <CircleCheckBig className="h-4 w-4 text-emerald-500" />
                    ) : state?.status === "error" ? (
                      <CircleAlert className="h-4 w-4 text-red-500" />
                    ) : (
                      <CircleAlert className="h-4 w-4 text-muted-foreground/50" />
                    )}
                    <span className="font-mono text-sm">{model}</span>
                    {isDefault && (
                      <Badge variant="secondary" className="text-[10px]">
                        默认
                      </Badge>
                    )}
                  </div>
                  <div className="text-xs">
                    {state?.status === "testing" ? (
                      <span className="text-blue-600">测试中...</span>
                    ) : state?.status === "success" && state.result ? (
                      <span className="text-emerald-600 font-mono">
                        {state.result.latency_ms}ms
                      </span>
                    ) : state?.status === "error" && state.result ? (
                      <span
                        className="text-red-600 cursor-pointer underline"
                        onClick={(e) => {
                          e.stopPropagation();
                          setExpandedError(expandedError === model ? null : model);
                        }}
                      >
                        {statusLabels[state.result.status]}
                      </span>
                    ) : (
                      <span className="text-muted-foreground">待测试</span>
                    )}
                  </div>
                </div>

                {state?.status === "error" && expandedError === model && state.result && (
                  <div className="px-3 py-2 bg-red-50 border-t text-xs">
                    <div className="font-medium text-red-700 mb-1">
                      {model} 错误详情
                    </div>
                    <div className="font-mono text-red-600 whitespace-pre-wrap">
                      {state.result.message}
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {successCount > 0 && (
        <div className="flex items-center justify-between text-xs px-3 py-2 bg-emerald-50 border border-emerald-200 rounded-lg">
          <span>
            测试结果: {successCount}/{models.length} 通过
          </span>
          {avgLatency !== null && (
            <span className="text-emerald-600 font-mono">
              平均延迟: {avgLatency}ms
            </span>
          )}
        </div>
      )}
    </div>
  );
}
