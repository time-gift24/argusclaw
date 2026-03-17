"use client";

import { useChatStore } from "@/lib/chat-store";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuPortal,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HugeiconsIcon } from "@hugeicons/react";
import { UnfoldMoreIcon } from "@hugeicons/core-free-icons";

export function ProviderSelector() {
  const providers = useChatStore((state) => state.providers);
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

  // Find the selected provider
  const selectedProvider = selectedProviderPreferenceId
    ? providers.find((p) => p.id === selectedProviderPreferenceId)
    : null;

  // Get the effective model name for display
  const effectiveModel =
    selectedModelOverride ??
    selectedProvider?.default_model ??
    "默认模型";

  // Display text: Provider name / Model name
  const displayText = selectedProvider
    ? `${selectedProvider.display_name} / ${effectiveModel}`
    : `系统默认 / ${effectiveModel}`;

  // Handle selecting a model from a provider
  const handleSelectModel = (providerId: string | null, model: string) => {
    void selectProviderPreference(providerId);
    void selectModelOverride(model);
  };

  // Get default provider's default model for "系统默认" option
  const defaultProvider = providers.find((p) => p.is_default);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className="flex h-7 items-center gap-1 rounded-md border border-input bg-background px-2 py-1 text-xs outline-none hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-ring/30 data-[popup-open]:bg-accent"
        render={
          <button type="button">
            <span className="max-w-[200px] truncate">{displayText}</span>
            <HugeiconsIcon
              icon={UnfoldMoreIcon}
              strokeWidth={2}
              className="size-3.5 text-muted-foreground"
            />
          </button>
        }
      />
      <DropdownMenuPortal>
        <DropdownMenuContent align="start" className="min-w-48">
          {/* 系统默认 option with nested models */}
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <span className={selectedProvider ? "" : "font-medium"}>
                系统默认
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuPortal>
              <DropdownMenuSubContent>
                <DropdownMenuLabel>选择模型</DropdownMenuLabel>
                {defaultProvider?.models.map((model) => (
                  <DropdownMenuItem
                    key={model}
                    onClick={() => handleSelectModel(null, model)}
                  >
                    {model}
                    {model === defaultProvider.default_model && (
                      <span className="ml-auto text-muted-foreground text-[10px]">
                        默认
                      </span>
                    )}
                    {!selectedProviderPreferenceId &&
                      model === selectedModelOverride && (
                        <HugeiconsIcon
                          icon={UnfoldMoreIcon}
                          strokeWidth={2}
                          className="ml-auto size-3"
                        />
                      )}
                  </DropdownMenuItem>
                ))}
                {!defaultProvider && (
                  <DropdownMenuItem disabled>
                    无可用模型
                  </DropdownMenuItem>
                )}
              </DropdownMenuSubContent>
            </DropdownMenuPortal>
          </DropdownMenuSub>

          <DropdownMenuSeparator />

          {/* Provider list with nested models */}
          {providers.map((provider) => (
            <DropdownMenuSub key={provider.id}>
              <DropdownMenuSubTrigger>
                <span
                  className={
                    selectedProviderPreferenceId === provider.id
                      ? "font-medium"
                      : ""
                  }
                >
                  {provider.display_name}
                </span>
              </DropdownMenuSubTrigger>
              <DropdownMenuPortal>
                <DropdownMenuSubContent>
                  <DropdownMenuLabel>选择模型</DropdownMenuLabel>
                  {provider.models.map((model) => (
                    <DropdownMenuItem
                      key={model}
                      onClick={() => handleSelectModel(provider.id, model)}
                    >
                      {model}
                      {model === provider.default_model && (
                        <span className="ml-auto text-muted-foreground text-[10px]">
                          默认
                        </span>
                      )}
                      {selectedProviderPreferenceId === provider.id &&
                        model === selectedModelOverride && (
                          <span className="ml-auto text-primary">✓</span>
                        )}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuSubContent>
              </DropdownMenuPortal>
            </DropdownMenuSub>
          ))}
        </DropdownMenuContent>
      </DropdownMenuPortal>
    </DropdownMenu>
  );
}
