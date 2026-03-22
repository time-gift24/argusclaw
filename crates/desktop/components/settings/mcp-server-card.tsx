"use client";

import * as React from "react";
import { Pencil, Trash2, Plug, Check, X } from "lucide-react";
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
import type { McpServerSummary, ConnectionTestResult } from "@/lib/tauri";

interface McpServerCardProps {
  server: McpServerSummary;
  onEdit: (id: number) => void;
  onDelete: (id: number) => void;
  onTestConnection: (id: number) => void;
  testResult?: ConnectionTestResult;
  isTesting?: boolean;
}

export function McpServerCard({
  server,
  onEdit,
  onDelete,
  onTestConnection,
  testResult,
  isTesting = false,
}: McpServerCardProps) {
  const hasResult = !!testResult;
  const isSuccess = testResult?.success;
  const isFailure = hasResult && !testResult.success;

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base flex items-center gap-2">
            <Plug className="h-5 w-5 text-muted-foreground" />
            <span>{server.display_name}</span>
            {!server.enabled && (
              <Badge variant="outline" className="border-amber-300 text-amber-700">
                已禁用
              </Badge>
            )}
          </CardTitle>
          <CardDescription className="text-xs">
            ID: {server.id}
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <div className="flex justify-between">
          <span className="text-muted-foreground">名称:</span>
          <span className="font-mono text-xs bg-muted px-2 py-1 rounded">
            {server.name}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">传输方式:</span>
          <Badge variant="secondary" className="text-xs">
            {server.server_type === "stdio" ? "标准 I/O" : "HTTP"}
          </Badge>
        </div>
        {hasResult && (
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
                {isTesting ? (
                  "测试中..."
                ) : isSuccess ? (
                  <>
                    <Check className="mr-1 h-3 w-3" />
                    连接成功
                  </>
                ) : (
                  <>
                    <X className="mr-1 h-3 w-3" />
                    连接失败
                  </>
                )}
              </Badge>
              {testResult.success && testResult.tool_count > 0 && (
                <span className="text-xs text-muted-foreground">
                  {testResult.tool_count} 个工具
                </span>
              )}
            </div>
          </div>
        )}
        {hasResult && !testResult.success && testResult.error_message && (
          <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            {testResult.error_message}
          </div>
        )}
      </CardContent>
      <CardFooter className="flex flex-wrap gap-2">
        <Button
          size="sm"
          onClick={() => onTestConnection(server.id)}
          disabled={isTesting || !server.enabled}
        >
          <Plug className="h-3 w-3 mr-1" />
          {isTesting ? "测试中..." : "测试连接"}
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => onEdit(server.id)}
        >
          <Pencil className="h-3 w-3 mr-1" />
          编辑
        </Button>
        <Button
          size="sm"
          variant="destructive"
          onClick={() => onDelete(server.id)}
        >
          <Trash2 className="h-3 w-3 mr-1" />
          删除
        </Button>
      </CardFooter>
    </Card>
  );
}
