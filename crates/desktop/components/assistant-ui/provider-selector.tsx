"use client";

import * as React from "react";
import { Cloud, Check, ChevronRight, Cpu, Zap, Globe } from "lucide-react";
import { useChatStore } from "@/lib/chat-store";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

export function ProviderSelector() {
  const providers = useChatStore((state) => state.providers);
  const activeSession = useChatStore((state) =>
    state.activeSessionKey ? state.sessionsByKey[state.activeSessionKey] ?? null : null,
  );
  const selectedProviderPreferenceId = useChatStore(
    (state) => state.selectedProviderPreferenceId,
  );
  const selectedModelOverride = useChatStore(
    (state) => state.selectedModelOverride,
  );
  const selectProviderPreference = useChatStore(
    (state) => state.selectProviderPreference,
  );
  const selectModelOverride = useChatStore(
    (state) => state.selectModelOverride,
  );

  // Find the current provider and model
  const currentProviderId =
    activeSession?.effectiveProviderId ?? selectedProviderPreferenceId;
  const currentProvider = currentProviderId
    ? providers.find((p) => p.id === currentProviderId)
    : providers.find((p) => p.is_default);

  const currentModel = activeSession
    ? currentProvider?.default_model ?? "未知模型"
    : selectedModelOverride ?? currentProvider?.default_model ?? "未知模型";

  // Handle selecting a model
  const handleSelectModel = (providerId: number, model: string) => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;

    void selectProviderPreference(providerId);
    // If it's the default model, we don't need an override
    void selectModelOverride(model === provider.default_model ? null : model);
  };

  const trigger = (
    <button
      type="button"
      className="flex h-8 items-center gap-2 px-3 rounded-full bg-muted/50 hover:bg-muted transition-all border border-transparent hover:border-muted-foreground/20 group outline-none focus-visible:ring-2 focus-visible:ring-primary/20"
    >
      <div className="flex h-4 w-4 items-center justify-center rounded-full bg-primary/10 text-primary group-hover:bg-primary group-hover:text-primary-foreground transition-colors">
        <Cpu className="size-3" />
      </div>
      <div className="flex items-center gap-1.5 min-w-0">
        <span className="max-w-[100px] truncate text-[11px] font-bold tracking-tight opacity-70">
          {currentProvider?.display_name ?? "选择提供者"}
        </span>
        <span className="text-[10px] opacity-30 font-bold">/</span>
        <span className="max-w-[120px] truncate text-[11px] font-bold tracking-tight text-primary">
          {currentModel}
        </span>
      </div>
      <ChevronRight className="size-3 opacity-40 group-hover:opacity-100 group-hover:translate-x-0.5 transition-all" />
    </button>
  );

  if (providers.length === 0) return null;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        {trigger}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64 p-2 rounded-[24px] shadow-2xl border-none bg-background/95 backdrop-blur-xl">
        <div className="px-3 py-2 mb-1">
          <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest opacity-60">
            Available AI Models
          </p>
        </div>
        
        <div className="max-h-[400px] overflow-y-auto custom-scrollbar pr-1">
          {providers.map((provider, providerIndex) => (
            <div key={provider.id}>
              {providerIndex > 0 && <DropdownMenuSeparator className="my-2 opacity-50" />}
              
              <div className="px-3 py-1.5 flex items-center gap-2">
                <div className="p-1 rounded-md bg-muted text-muted-foreground">
                  <Globe className="size-3" />
                </div>
                <span className="text-[11px] font-bold truncate flex-1">{provider.display_name}</span>
                {provider.is_default && (
                  <Badge className="text-[8px] h-3.5 px-1 bg-primary/10 text-primary border-none font-bold uppercase">Default</Badge>
                )}
              </div>

              <div className="grid gap-0.5 mt-1">
                {provider.models.map((model) => {
                  const isModelSelected = 
                    currentProviderId === provider.id && 
                    (activeSession
                      ? model === currentProvider?.default_model
                      : selectedModelOverride === model || (selectedModelOverride === null && model === provider.default_model));
                  
                  // Special case: if nothing selected, use default provider's default model
                  const isEffectivelySelected =
                    currentProviderId == null &&
                    provider.is_default &&
                    model === provider.default_model;

                  const active = isModelSelected || isEffectivelySelected;

                  return (
                    <DropdownMenuItem
                      key={`${provider.id}-${model}`}
                      onClick={() => handleSelectModel(provider.id, model)}
                      className={cn(
                        "flex items-center gap-2 px-3 py-2 rounded-xl cursor-pointer transition-all",
                        active ? "bg-primary/5 text-primary" : "hover:bg-muted"
                      )}
                    >
                      <div className={cn(
                        "size-1.5 rounded-full shrink-0",
                        active ? "bg-primary" : "bg-muted-foreground/30"
                      )} />
                      <span className={cn(
                        "flex-1 truncate text-xs font-medium",
                        active ? "font-bold" : ""
                      )}>
                        {model}
                      </span>
                      {model === provider.default_model && (
                        <Zap className="size-3 text-amber-500 opacity-70 shrink-0" />
                      )}
                      {active && (
                        <Check className="size-3.5 text-primary shrink-0" />
                      )}
                    </DropdownMenuItem>
                  );
                })}
              </div>
            </div>
          ))}
        </div>

        <DropdownMenuSeparator className="my-2" />
        <div className="px-3 py-2">
          <p className="text-[9px] text-muted-foreground leading-tight italic opacity-60">
            * 默认模型由图标 <Zap className="inline size-2.5" /> 标识。
          </p>
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
