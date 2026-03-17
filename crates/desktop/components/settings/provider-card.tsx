"use client";

import * as React from "react";
import { Cloud, Pencil, Trash2, Check, Activity } from "lucide-react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import type { ProviderSecretStatus, ProviderTestResult } from "@/lib/tauri";

export interface LlmProviderSummary {
  id: string;
  kind: string;
  display_name: string;
  base_url: string;
  models: string[];
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

interface ProviderCardProps {
  provider: LlmProviderSummary;
  onDelete: (id: string) => void;
  onSetDefault: (id: string) => void;
  onTestConnection: (id: string) => void;
  onViewStatus: (id: string) => void;
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
  const hasResult = !!testResult;
  const isSuccess = testResult?.status === "success";
  const isFailure = !!testResult && failureStatuses.has(testResult.status);
  const requiresReentry = provider.secret_status === "requires_reentry";

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base flex items-center gap-2">
            <Cloud className="h-5 w-5 text-muted-foreground" />
            <span>{provider.display_name}</span>
            {provider.is_default && (
              <Badge
                variant="default"
                className="bg-primary text-primary-foreground text-xs"
              >
                <Check className="mr-1 h-3 w-3" />
                Default
              </Badge>
            )}
            {requiresReentry && (
              <Badge variant="outline" className="border-amber-300 text-amber-700">
                需要重新填写 API Key
              </Badge>
            )}
          </CardTitle>
          <CardDescription className="text-xs">{provider.id}</CardDescription>
        </div>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <div className="flex justify-between">
          <span className="text-muted-foreground">Kind:</span>
          <span className="font-mono text-xs bg-muted px-2 py-1 rounded">
            {provider.kind}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Default Model:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded">
            {provider.default_model}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Models:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded max-w-[200px]">
            {provider.models.join(", ")}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Base URL:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded max-w-[200px]">
            {provider.base_url}
          </span>
        </div>
        {requiresReentry && (
          <div className="rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-xs text-amber-800">
            当前保存的密钥无法解密，编辑后重新填写 API Key 才能继续使用。
          </div>
        )}
        {(isTesting || hasResult) && (
          <div className="flex items-center justify-between gap-3 rounded-md border border-border/60 bg-muted/30 px-3 py-2">
            <div className="flex items-center gap-2">
              <Badge
                variant="outline"
                className={
                  isTesting
                    ? "border-sky-200 text-sky-700"
                    : isSuccess
                      ? "border-emerald-200 text-emerald-700"
                      : isFailure
                        ? "border-destructive/30 text-destructive"
                        : ""
                }
              >
                {isTesting ? "运行中" : isSuccess ? "成功" : "失败"}
              </Badge>
              {hasResult ? (
                <Button
                  size="sm"
                  variant="link"
                  className="h-auto px-0 text-xs"
                  onClick={() => onViewStatus(provider.id)}
                >
                  查看状态
                </Button>
              ) : null}
            </div>
            {testResult?.latency_ms !== undefined ? (
              <span className="font-mono text-[11px] text-muted-foreground">
                {testResult.latency_ms} ms
              </span>
            ) : null}
          </div>
        )}
      </CardContent>
      <CardFooter className="flex flex-wrap gap-2">
        <Button
          size="sm"
          onClick={() => onTestConnection(provider.id)}
          disabled={isTesting || requiresReentry}
        >
          <Activity className="h-3 w-3 mr-1" />
          测试连接
        </Button>
        <Button size="sm" variant="outline" onClick={() => router.push(`/settings/providers/${provider.id}`)}>
          <Pencil className="h-3 w-3 mr-1" />
          Edit
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => onSetDefault(provider.id)}
        >
          <Check className="h-3 w-3 mr-1" />
          Set Default
        </Button>
        <Button
          size="sm"
          variant="destructive"
          onClick={() => onDelete(provider.id)}
          disabled={provider.is_default}
        >
          <Trash2 className="h-3 w-3 mr-1" />
          Delete
        </Button>
      </CardFooter>
    </Card>
  );
}
