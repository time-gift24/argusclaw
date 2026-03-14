import { Thread } from "@/components/assistant-ui/thread";
import { TauriRuntimeProvider } from "@/components/assistant-ui/tauri-runtime";

export default function Page() {
  return (
    <main className="flex h-svh flex-col">
      <TauriRuntimeProvider>
        <Thread />
      </TauriRuntimeProvider>
    </main>
  );
}
