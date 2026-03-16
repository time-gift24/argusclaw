"use client";

import { useChatStore } from "@/lib/chat-store";

export function ProviderSelector() {
  const providers = useChatStore((state) => state.providers);
  const providerModels = useChatStore((state) => state.providerModels);
  const selectedProviderPreferenceId = useChatStore((state) => state.selectedProviderPreferenceId);
  const selectProviderPreference = useChatStore((state) => state.selectProviderPreference);

  // Build options list: default option + provider-model combinations
  const options = [
    { id: "__default__", label: "系统默认" },
    ...providerModels.map((pm) => ({
      id: `${pm.providerId}:${pm.modelId}`,
      label: `${pm.providerName} - ${pm.modelName}`,
    })),
    // Also include providers without models as fallback
    ...providers
      .filter((p) => !providerModels.some((pm) => pm.providerId === p.id))
      .map((p) => ({
        id: p.id,
        label: p.display_name,
      })),
  ];

  return (
    <select
      value={selectedProviderPreferenceId ?? "__default__"}
      onChange={(e) => void selectProviderPreference(e.target.value === "__default__" ? null : e.target.value)}
      className="rounded-md border border-input bg-background px-2 py-1 text-sm"
    >
      {options.map((option) => (
        <option key={option.id} value={option.id}>
          {option.label}
        </option>
      ))}
    </select>
  );
}
