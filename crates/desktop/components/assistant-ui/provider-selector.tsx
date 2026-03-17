"use client";

import { useChatStore } from "@/lib/chat-store";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuPortal,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HugeiconsIcon } from "@hugeicons/react";
import { UnfoldMoreIcon, Tick02Icon } from "@hugeicons/core-free-icons";

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
    : providers.find((p) => p.is_default)
      ? `${providers.find((p) => p.is_default)!.display_name} / ${effectiveModel}`
      : `无提供商 / ${effectiveModel}`;

  // Handle selecting a model from a provider
  const handleSelectModel = (providerId: string, model: string) => {
    void selectProviderPreference(providerId);
    void selectModelOverride(model);
  };

  // Check if a specific item is selected
  const isSelected = (providerId: string, model: string) => {
    // If no preference set, use the default provider
    const effectiveProviderId = selectedProviderPreferenceId ?? providers.find((p) => p.is_default)?.id;
    return effectiveProviderId === providerId && selectedModelOverride === model;
  };

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
        <DropdownMenuContent align="start" className="max-h-80 min-w-48 overflow-y-auto">
          {providers.map((provider, index) => (
            <div key={provider.id}>
              {index > 0 && <DropdownMenuSeparator />}
              <DropdownMenuLabel className="flex items-center gap-1 text-muted-foreground">
                {provider.display_name}
                {provider.is_default && (
                  <span className="text-muted-foreground text-[10px]">(默认)</span>
                )}
              </DropdownMenuLabel>
              {provider.models.map((model) => (
                <DropdownMenuItem
                  key={`${provider.id}-${model}`}
                  onClick={() => handleSelectModel(provider.id, model)}
                  className="pl-6"
                >
                  <span className="flex-1 truncate">{model}</span>
                  {model === provider.default_model && (
                    <span className="text-muted-foreground text-[10px]">默认</span>
                  )}
                  {isSelected(provider.id, model) && (
                    <HugeiconsIcon
                      icon={Tick02Icon}
                      strokeWidth={2}
                      className="size-3.5 text-primary"
                    />
                  )}
                </DropdownMenuItem>
              ))}
            </div>
          ))}
          {providers.length === 0 && (
            <DropdownMenuItem disabled>
              无可用提供商
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenuPortal>
    </DropdownMenu>
  );
}
