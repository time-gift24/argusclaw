"use client";

import * as React from "react";
import { CircleAlert, CircleCheckBig, LoaderCircle } from "lucide-react";
import type { LlmProviderSummary, ProviderTestResult } from "@/lib/tauri";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface ProviderTestDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  provider: LlmProviderSummary | null;
  result?: ProviderTestResult | null;
  testing?: boolean;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  onRetest: () => void;
}

function formatCheckedAt(value: string) {
  return new Date(value).toLocaleString("zh-CN", {
    hour12: false,
  });
}

export function ProviderTestDialog({
  open,
  onOpenChange,
  provider,
  result,
  testing = false,
  selectedModel,
  onModelChange,
  onRetest,
}: ProviderTestDialogProps) {
  const statusTone = testing
    ? "border-sky-200 text-sky-700"
    : result?.status === "success"
      ? "border-emerald-200 text-emerald-700"
      : "border-destructive/30 text-destructive";
  const statusLabel = testing
    ? "运行中"
    : result?.status === "success"
      ? "成功"
      : "失败";

  const displayModel = selectedModel ?? provider?.default_model ?? result?.model ?? "-";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Provider 连接状态</DialogTitle>
          <DialogDescription>
            查看最近一次测试结果，必要时可以直接重新测试。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="rounded-lg border border-border/60 bg-muted/30 p-4">
            <div className="flex items-start justify-between gap-3">
              <div className="space-y-1">
                <p className="text-sm font-medium">
                  {provider?.display_name ?? "未选择 Provider"}
                </p>
                <p className="font-mono text-[11px] text-muted-foreground">
                  {provider?.id ?? result?.provider_id ?? "unknown"}
                </p>
              </div>
              <Badge variant="outline" className={statusTone}>
                {testing ? (
                  <LoaderCircle className="mr-1 h-3 w-3 animate-spin" />
                ) : result?.status === "success" ? (
                  <CircleCheckBig className="mr-1 h-3 w-3" />
                ) : (
                  <CircleAlert className="mr-1 h-3 w-3" />
                )}
                {statusLabel}
              </Badge>
            </div>

            <div className="mt-4 grid gap-3 text-xs">
              <div className="flex items-start justify-between gap-3">
                <span className="text-muted-foreground">Model</span>
                <span className="font-mono text-right">
                  {displayModel}
                </span>
              </div>
              <div className="flex items-start justify-between gap-3">
                <span className="text-muted-foreground">Base URL</span>
                <span className="max-w-[300px] break-all font-mono text-right">
                  {provider?.base_url ?? result?.base_url ?? "-"}
                </span>
              </div>
            </div>
          </div>

          {provider && provider.models.length > 0 && onModelChange && (
            <div className="space-y-2">
              <Label className="text-xs text-muted-foreground">选择测试模型</Label>
              <Select
                value={selectedModel || provider.default_model}
                onValueChange={(value) => {
                  if (value) onModelChange(value);
                }}
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="选择模型" />
                </SelectTrigger>
                <SelectContent>
                  {provider.models.map((model) => (
                    <SelectItem key={model} value={model}>
                      {model}
                      {model === provider.default_model && " (默认)"}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            {/* 左侧：消息和基本详情 */}
            <div className="space-y-4">
              <div className="rounded-lg border border-border/60 p-4">
                <p className="mb-2 text-xs font-medium text-muted-foreground">
                  详情
                </p>
                <p className="text-sm">
                  {testing
                    ? "正在测试当前 provider 的连接状态，请稍候。"
                    : (result?.message ?? "点击重新测试来查看当前状态。")}
                </p>
              </div>

              <div className="grid gap-3 rounded-lg border border-border/60 p-4 text-xs">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-mono text-muted-foreground">
                    latency_ms
                  </span>
                  <span className="font-mono">
                    {result ? `${result.latency_ms} ms` : "-"}
                  </span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span className="font-mono text-muted-foreground">
                    checked_at
                  </span>
                  <span className="font-mono">
                    {result ? formatCheckedAt(result.checked_at) : "-"}
                  </span>
                </div>
              </div>
            </div>

            {/* 右侧：请求和响应 */}
            {result && (
              <div className="space-y-2">
                {result.request != null && (
                  <Collapsible defaultOpen={true}>
                    <CollapsibleTrigger className="flex w-full items-center justify-between rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-xs font-medium hover:bg-muted/50">
                      <span>请求</span>
                    </CollapsibleTrigger>
                    <CollapsibleContent>
                      <pre className="mt-1 max-h-[200px] overflow-x-auto rounded-lg border border-border/60 bg-muted/30 p-3 font-mono text-[11px] leading-relaxed">
                        {(() => {
                          try {
                            return JSON.stringify(JSON.parse(result.request!), null, 2);
                          } catch {
                            return result.request;
                          }
                        })()}
                      </pre>
                    </CollapsibleContent>
                  </Collapsible>
                )}

                {result.response != null && result.status === "success" && (
                  <Collapsible defaultOpen={true}>
                    <CollapsibleTrigger className="flex w-full items-center justify-between rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-xs font-medium hover:bg-muted/50">
                      <span>响应</span>
                    </CollapsibleTrigger>
                    <CollapsibleContent>
                      <pre className="mt-1 max-h-[200px] overflow-x-auto rounded-lg border border-border/60 bg-muted/30 p-3 font-mono text-[11px] leading-relaxed">
                        {result.response || "(空响应)"}
                      </pre>
                    </CollapsibleContent>
                  </Collapsible>
                )}
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
          <Button onClick={onRetest} disabled={!provider || testing}>
            {testing ? "正在测试" : "重新测试"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
