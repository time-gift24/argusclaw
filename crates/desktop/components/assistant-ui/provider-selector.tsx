"use client";

import { useChatStore } from "@/lib/chat-store";

export function ProviderSelector() {
  const providers = useChatStore((state) => state.providers);
  const selectedProviderPreferenceId = useChatStore((state) => state.selectedProviderPreferenceId);
  const selectProviderPreference = useChatStore((state) => state.selectProviderPreference);

  return (
    <select
      value={selectedProviderPreferenceId ?? "__default__"}
      onChange={(e) => void selectProviderPreference(e.target.value === "__default__" ? null : e.target.value)}
      className="rounded-md border border-input bg-background px-2 py-1 text-sm"
    >
      <option value="__default__">系统默认</option>
      {providers.map((provider) => (
        <option key={provider.id} value={provider.id}>
          {provider.display_name}
        </option>
      ))}
    </select>
  );
}
