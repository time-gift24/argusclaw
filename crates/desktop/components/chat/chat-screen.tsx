"use client";

import * as React from "react";
import { AssistantRuntimeProvider } from "@assistant-ui/react";

import { Thread } from "@/components/assistant-ui/thread";
import { ThreadMonitorScreen } from "@/components/thread-monitor/thread-monitor-screen";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useChatRuntime } from "@/lib/chat-runtime";
import { useChatStore } from "@/lib/chat-store";

export function ChatScreen() {
  const runtime = useChatRuntime();
  const initialize = useChatStore((state) => state.initialize);

  React.useEffect(() => {
    void initialize();
  }, [initialize]);

  return (
    <Tabs defaultValue="chat" className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="px-4 pt-4">
        <TabsList className="bg-muted/60 shadow-sm">
          <TabsTrigger value="chat">Chat</TabsTrigger>
          <TabsTrigger value="threads">Threads</TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="chat" className="m-0 flex min-h-0 flex-1">
        <AssistantRuntimeProvider runtime={runtime}>
          <Thread />
        </AssistantRuntimeProvider>
      </TabsContent>
      <TabsContent value="threads" className="m-0 flex min-h-0 flex-1">
        <ThreadMonitorScreen />
      </TabsContent>
    </Tabs>
  );
}
