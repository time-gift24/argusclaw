"use client";

import * as React from "react";
import { Cloud, Pencil, Trash2, Check, Activity, Globe, Cpu } from "lucide-react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import type {
  LlmProviderSummary,
  ProviderTestResult,
} from "@/lib/tauri";

interface ProviderCardProps {
  provider: LlmProviderSummary;
  onDelete: (id: number) => void;
  onSetDefault: (id: number) => void;
  onTestConnection: (id: number) => void;
  onViewStatus: (id: number) => void;
  testResult?: ProviderTestResult;
  isTesting?: boolean;
}

const failureStatuses = new Set<ProviderTestResult["status"]>([
  "auth_failed",
  "model_not_available",
  "rate_limited",
  "request_failed",
  "invalid_response",
  "provider_not_found",
  "unsupported_provider_kind",
]);

export function ProviderCard({
  provider,
  onDelete,
  onSetDefault,
  onTestConnection,
  onViewStatus,
  testResult,
  isTesting = false,
}: ProviderCardProps) {
  const router = useRouter();
  const isSuccess = testResult?.status === "success";
  const isFailure = !!testResult && failureStatuses.has(testResult.status);
  const requiresReentry = provider.secret_status === "requires_reentry";

  return (
    <Card className="group overflow-hidden border-muted/60 transition-all hover:border-primary/30 hover:shadow-md bg-background">
      <div className="flex flex-col p-4 gap-4">
        {/* Top: Header Group */}
        <div className="flex items-center justify-between gap-3 min-w-0">
          <div className="flex items-center gap-3 min-w-0">
            <div className="rounded-lg bg-primary/5 p-2 text-primary shrink-0 transition-colors group-hover:bg-primary group-hover:text-primary-foreground">
              <Cloud className="h-4 w-4" />
            </div>
            <div className="flex flex-col min-w-0">
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold truncate leading-none">{provider.display_name}</h3>
                {provider.is_default && (
                  <Badge className="text-[9px] h-3.5 px-1 bg-primary/10 text-primary border-none shadow-none font-bold">
                    默认
                  </Badge>
                )}
              </div>
              <p className="text-[11px] text-muted-foreground font-mono mt-1.5 leading-none opacity-50">
                ID: {provider.id}
              </p>
            </div>
          </div>

          <div className="shrink-0">
            {isTesting ? (
              <Badge variant="outline" className="text-[9px] h-4 px-1.5 border-sky-200 text-sky-600 animate-pulse font-bold uppercase">测试中</Badge>
            ) : isSuccess ? (
              <Badge
                variant="outline"
                className="text-[9px] h-4 px-1.5 border-emerald-200 text-emerald-600 cursor-pointer hover:bg-emerald-50 font-bold uppercase"
                aria-label="查看状态"
                title="查看状态"
                onClick={() => onViewStatus(provider.id)}
              >
                在线 · {testResult?.latency_ms}ms
              </Badge>
            ) : isFailure ? (
              <Badge
                variant="outline"
                className="text-[9px] h-4 px-1.5 border-destructive/30 text-destructive cursor-pointer hover:bg-destructive/5 font-bold uppercase"
                aria-label="查看状态"
                title="查看状态"
                onClick={() => onViewStatus(provider.id)}
              >
                连接失败
              </Badge>
            ) : (
              <Badge variant="outline" className="text-[9px] h-4 px-1.5 border-muted/60 text-muted-foreground font-bold uppercase">未测试</Badge>
            )}
          </div>
        </div>

        {/* Middle: Info Grid */}
        <div className="grid grid-cols-2 gap-4 pt-3 border-t border-muted/30">
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center gap-1.5 text-[9px] font-bold text-muted-foreground/50 uppercase tracking-wider leading-none">
              <Globe className="h-2.5 w-2.5" />
              接入地址
            </div>
            <span className="text-[11px] font-medium truncate leading-none">{provider.base_url}</span>
          </div>
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center gap-1.5 text-[9px] font-bold text-muted-foreground/50 uppercase tracking-wider leading-none">
              <Cpu className="h-2.5 w-2.5" />
              默认模型
            </div>
            <span className="text-[11px] font-mono font-medium truncate leading-none">{provider.default_model}</span>
          </div>
        </div>

        {requiresReentry && (
          <div className="rounded-xl bg-amber-50 p-3 border border-amber-100">
            <p className="text-[10px] text-amber-700 leading-normal flex items-start gap-2 font-medium">
              <span className="shrink-0 bg-amber-200 text-amber-800 rounded-full w-3.5 h-3.5 flex items-center justify-center text-[8px] font-bold">!</span>
              密钥已失效，请重新编辑并填写 API Key 才能继续使用。
            </p>
          </div>
        )}

        {/* Bottom: Actions */}
        <div className="flex items-center justify-between gap-2 pt-1">
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-8 text-[11px] px-3 rounded-lg hover:bg-primary/5 hover:text-primary transition-colors font-medium"
              onClick={() => onTestConnection(provider.id)}
              disabled={isTesting || requiresReentry}
            >
              <Activity className="h-3.5 w-3.5 mr-1.5" />
              测试连接
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-8 text-[11px] px-3 rounded-lg hover:bg-muted/80 transition-colors font-medium text-muted-foreground"
              onClick={() => onSetDefault(provider.id)}
              disabled={provider.is_default}
            >
              <Check className="h-3.5 w-3.5 mr-1.5" />
              设为默认
            </Button>
          </div>

          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 rounded-lg hover:bg-muted/80 transition-colors"
              onClick={() => router.push(`/settings/providers/edit?id=${provider.id}`)}
            >
              <Pencil className="h-3.5 w-3.5" />
              <span className="sr-only">编辑</span>
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 rounded-lg hover:bg-destructive/5 hover:text-destructive transition-colors text-muted-foreground hover:text-destructive"
              onClick={() => onDelete(provider.id)}
              disabled={provider.is_default}
            >
              <Trash2 className="h-3.5 w-3.5" />
              <span className="sr-only">删除</span>
            </Button>
          </div>
        </div>
      </div>
    </Card>
  );
}
