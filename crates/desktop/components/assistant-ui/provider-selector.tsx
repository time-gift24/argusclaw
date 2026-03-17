"use client";

import { useChatStore } from "@/lib/chat-store";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

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
  const selectModelOverride = useChatStore((state) => state.selectModelOverride);

  // Find the selected provider to get its models
  const selectedProvider = selectedProviderPreferenceId
    ? providers.find((p) => p.id === selectedProviderPreferenceId)
    : null;

  const handleProviderChange = (value: string | null) => {
    const newProviderId = value === "__default__" ? null : value;
    void selectProviderPreference(newProviderId);
    // Reset model override when provider changes
    void selectModelOverride(null);
  };

  const handleModelChange = (value: string | null) => {
    void selectModelOverride(value === "__default__" ? null : value);
  };

  return (
    <div className="flex items-center gap-2">
      <Select
        value={selectedProviderPreferenceId ?? "__default__"}
        onValueChange={handleProviderChange}
      >
        <SelectTrigger className="h-7 w-auto min-w-[100px] border-input bg-background px-2 py-1 text-xs">
          <SelectValue placeholder="系统默认" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__default__">系统默认</SelectItem>
          {providers.map((provider) => (
            <SelectItem key={provider.id} value={provider.id}>
              {provider.display_name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Model selector - only show when a provider is selected */}
      {selectedProvider && selectedProvider.models.length > 0 && (
        <Select
          value={selectedModelOverride ?? "__default__"}
          onValueChange={handleModelChange}
        >
          <SelectTrigger className="h-7 w-auto min-w-[80px] border-input bg-background px-2 py-1 text-xs">
            <SelectValue placeholder="默认" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__default__">
              {selectedProvider.default_model} (默认)
            </SelectItem>
            {selectedProvider.models
              .filter((m) => m !== selectedProvider.default_model)
              .map((model) => (
                <SelectItem key={model} value={model}>
                  {model}
                </SelectItem>
              ))}
          </SelectContent>
        </Select>
      )}
    </div>
  );
}
