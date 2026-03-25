"use client";

import * as React from "react";
import { CircleAlert, CircleCheckBig, Loader2, Activity, Globe, Cpu, Clock, MessageSquare, Terminal } from "lucide-react";
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
import { cn } from "@/lib/utils";

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
  const isSuccess = result?.status === "success";
  
  const statusTone = testing
    ? "border-sky-200 text-sky-600 bg-sky-50/50"
    : isSuccess
      ? "border-emerald-200 text-emerald-600 bg-emerald-50/50"
      : "border-destructive/30 text-destructive bg-destructive/5";
      
  const statusLabel = testing
    ? "运行中"
    : isSuccess
      ? "连接成功"
      : "连接失败";

  const displayModel = selectedModel ?? provider?.default_model ?? result?.model ?? "-";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[800px] p-0 overflow-hidden border-none shadow-2xl rounded-[28px] bg-background">
        {/* Header Area */}
        <div className="bg-muted/30 px-8 py-6 border-b border-muted/60">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <div className="bg-primary/10 p-2.5 rounded-2xl text-primary shadow-sm shadow-primary/5">
                <Activity className="h-5 w-5" />
              </div>
              <div className="space-y-0.5">
                <DialogTitle className="text-lg font-bold tracking-tight">连接连通性测试</DialogTitle>
                <DialogDescription className="text-xs font-medium text-muted-foreground uppercase tracking-widest opacity-70">
                  Connectivity Diagnostic / {provider?.display_name || "Unknown"}
                </DialogDescription>
              </div>
            </div>
            
            <Badge variant="outline" className={cn("px-2 py-0.5 h-6 text-[10px] font-bold uppercase tracking-tighter rounded-full border shadow-none", statusTone)}>
              {testing ? (
                <Loader2 className="mr-1.5 h-3 w-3 animate-spin" />
              ) : isSuccess ? (
                <CircleCheckBig className="mr-1.5 h-3 w-3" />
              ) : (
                <CircleAlert className="mr-1.5 h-3 w-3" />
              )}
              {statusLabel}
            </Badge>
          </div>
        </div>

        {/* Content Body */}
        <div className="p-8 space-y-8 overflow-y-auto max-h-[70vh] custom-scrollbar">
          {/* Quick Stats Grid */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            {/* Left: Configuration Info */}
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-[11px] font-bold text-muted-foreground uppercase tracking-widest px-1">
                <Terminal className="h-3.5 w-3.5" />
                Target Environment
              </div>
              <div className="bg-muted/20 rounded-2xl p-5 border border-muted/60 space-y-4 shadow-sm">
                <div className="flex justify-between items-start gap-4">
                  <span className="text-[11px] font-bold text-muted-foreground/60 uppercase">接入地址</span>
                  <span className="text-xs font-mono font-medium truncate max-w-[200px] bg-background/50 px-2 py-0.5 rounded border border-muted/40">
                    {provider?.base_url ?? result?.base_url ?? "-"}
                  </span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-[11px] font-bold text-muted-foreground/60 uppercase">延迟 (Latency)</span>
                  <span className="text-xs font-mono font-bold text-emerald-600">
                    {result ? `${result.latency_ms} ms` : "-"}
                  </span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-[11px] font-bold text-muted-foreground/60 uppercase">测试时间</span>
                  <span className="text-[11px] font-medium text-muted-foreground">
                    {result ? formatCheckedAt(result.checked_at) : "从未测试"}
                  </span>
                </div>
              </div>
            </div>

            {/* Right: Model Selection */}
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-[11px] font-bold text-muted-foreground uppercase tracking-widest px-1">
                <Cpu className="h-3.5 w-3.5" />
                Model Validation
              </div>
              <div className="bg-muted/20 rounded-2xl p-5 border border-muted/60 space-y-4 shadow-sm h-[calc(100%-2rem)]">
                <div className="space-y-2">
                  <Label className="text-[10px] font-bold text-muted-foreground/60 uppercase ml-1">测试目标模型</Label>
                  <Select
                    value={selectedModel || provider?.default_model}
                    onValueChange={(value) => onModelChange?.(value)}
                    disabled={testing}
                  >
                    <SelectTrigger className="h-10 bg-background border-muted/60 text-sm font-mono focus:ring-primary/20">
                      <SelectValue placeholder="选择模型" />
                    </SelectTrigger>
                    <SelectContent className="rounded-xl border-muted/60 shadow-xl">
                      {provider?.models.map((model) => (
                        <SelectItem key={model} value={model} className="text-xs font-mono">
                          {model} {model === provider.default_model && "(默认)"}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <p className="text-[10px] text-muted-foreground leading-relaxed px-1">
                  选择提供者支持的任意模型进行连通性测试。测试将发起一次最小 Token 消耗的 Chat Completion 请求。
                </p>
              </div>
            </div>
          </div>

          {/* Diagnostic Message */}
          <div className="space-y-4">
            <div className="flex items-center gap-2 text-[11px] font-bold text-muted-foreground uppercase tracking-widest px-1">
              <MessageSquare className="h-3.5 w-3.5" />
              System Diagnostics
            </div>
            <div className={cn("p-5 rounded-2xl border flex items-start gap-4", 
              isSuccess ? "bg-emerald-50/20 border-emerald-100" : "bg-muted/20 border-muted/60")}>
              <div className={cn("p-2 rounded-xl shrink-0 shadow-sm", 
                isSuccess ? "bg-emerald-100 text-emerald-600" : "bg-muted text-muted-foreground")}>
                {isSuccess ? <CircleCheckBig className="h-4 w-4" /> : <Activity className="h-4 w-4" />}
              </div>
              <div className="space-y-1 py-0.5 min-w-0">
                <p className={cn("text-[13px] font-bold leading-tight", isSuccess ? "text-emerald-700" : "text-foreground")}>
                  {testing ? "正在执行诊断程序..." : isSuccess ? "握手成功" : "诊断详情"}
                </p>
                <p className="text-xs text-muted-foreground leading-relaxed break-words">
                  {testing ? "正在建立加密连接并测试 API 响应，这通常需要 1-3 秒。" : (result?.message || "请点击下方的“重新发起测试”按钮。")}
                </p>
              </div>
            </div>
          </div>

          {/* Collapsible Payloads */}
          {result && (
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 pt-2">
              <Collapsible className="group">
                <CollapsibleTrigger asChild>
                  <Button variant="outline" className="w-full justify-between h-9 text-xs rounded-xl border-muted/60 bg-background hover:bg-muted transition-all">
                    <span className="flex items-center gap-2 font-bold uppercase tracking-wider text-[10px] text-muted-foreground">Request Payload</span>
                    <Terminal className="h-3 w-3 opacity-40 group-data-[state=open]:rotate-90 transition-transform" />
                  </Button>
                </CollapsibleTrigger>
                <CollapsibleContent className="animate-in fade-in slide-in-from-top-1 duration-200">
                  <pre className="mt-2 p-4 rounded-2xl bg-muted/30 border border-muted/60 font-mono text-[10px] overflow-auto max-h-64 leading-relaxed custom-scrollbar">
                    {(() => {
                      try {
                        return JSON.stringify(JSON.parse(result.request || "{}"), null, 2);
                      } catch {
                        return result.request;
                      }
                    })()}
                  </pre>
                </CollapsibleContent>
              </Collapsible>

              <Collapsible className="group">
                <CollapsibleTrigger asChild>
                  <Button variant="outline" className="w-full justify-between h-9 text-xs rounded-xl border-muted/60 bg-background hover:bg-muted transition-all">
                    <span className="flex items-center gap-2 font-bold uppercase tracking-wider text-[10px] text-muted-foreground">Response Body</span>
                    <Terminal className="h-3 w-3 opacity-40 group-data-[state=open]:rotate-90 transition-transform" />
                  </Button>
                </CollapsibleTrigger>
                <CollapsibleContent className="animate-in fade-in slide-in-from-top-1 duration-200">
                  <pre className="mt-2 p-4 rounded-2xl bg-muted/30 border border-muted/60 font-mono text-[10px] overflow-auto max-h-64 leading-relaxed custom-scrollbar">
                    {result.response || "(空响应)"}
                  </pre>
                </CollapsibleContent>
              </Collapsible>
            </div>
          )}
        </div>

        {/* Footer Area */}
        <div className="bg-muted/10 px-8 py-6 border-t border-muted/60 flex items-center justify-between">
          <p className="text-[10px] text-muted-foreground font-medium flex items-center gap-1.5">
            <Clock className="h-3 w-3 opacity-50" />
            测试仅检查 API 连通性，不会保留对话历史。
          </p>
          <div className="flex items-center gap-3">
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)} className="text-xs font-bold text-muted-foreground uppercase tracking-wider h-9">
              关闭
            </Button>
            <Button 
              size="sm" 
              onClick={onRetest} 
              disabled={!provider || testing} 
              className="h-9 px-6 text-xs font-bold shadow-lg shadow-primary/20"
            >
              {testing ? (
                <><Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" /> 测试中...</>
              ) : (
                "重新发起测试"
              )}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
