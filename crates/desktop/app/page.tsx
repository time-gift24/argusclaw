import { ChatScreen } from "@/components/chat/chat-screen";
import { ThreadSidebar } from "@/components/chat/thread-sidebar";

export default function Page() {
  return (
    <div className="flex h-full min-h-0">
      <ThreadSidebar />
      <ChatScreen />
    </div>
  );
}
