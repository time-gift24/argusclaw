"use client";

import * as React from "react";
import { Cloud, Pencil, Trash2, Check, Activity } from "lucide-react";
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
import type { ProviderTestResult } from "@/lib/tauri";

export interface LlmProviderSummary {
  id: string;
  kind: string;
  display_name: string;
  base_url: string;
  model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
}

interface ProviderCardProps {
  provider: LlmProviderSummary;
  onEdit: (id: string) => void;
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
  onEdit,
  onDelete,
  onSetDefault,
  onTestConnection,
  onViewStatus,
  testResult,
  isTesting = false,
}: ProviderCardProps) {
  const hasResult = !!testResult;
  const isSuccess = testResult?.status === "success";
  const isFailure = !!testResult && failureStatuses.has(testResult.status);

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
          <span className="text-muted-foreground">Model:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded">
            {provider.model}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Base URL:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded max-w-[200px]">
            {provider.base_url}
          </span>
        </div>
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
          disabled={isTesting}
        >
          <Activity className="h-3 w-3 mr-1" />
          测试连接
        </Button>
        <Button size="sm" variant="outline" onClick={() => onEdit(provider.id)}>
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
