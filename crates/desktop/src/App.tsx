import {
  Sidebar,
  SidebarContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { HugeiconsIcon } from "@hugeicons/react";
import { ChatIcon } from "@hugeicons/core-free-icons";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ChatPage } from "@/components/chat/ChatPage";

function App() {
  return (
    <TooltipProvider>
      <SidebarProvider defaultOpen={true}>
        <Sidebar variant="floating" collapsible="offcanvas">
          <SidebarHeader className="py-4">
            <div className="flex items-center justify-center">
              <span className="text-lg font-semibold">ArgusClaw</span>
            </div>
          </SidebarHeader>
          <SidebarContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton isActive>
                  <HugeiconsIcon icon={ChatIcon} />
                  <span>聊天</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarContent>
        </Sidebar>
        <SidebarInset>
          <header className="flex h-14 items-center gap-2 border-b px-4">
            <SidebarTrigger />
            <span className="text-sm font-medium">聊天</span>
          </header>
          <ChatPage />
        </SidebarInset>
      </SidebarProvider>
    </TooltipProvider>
  );
}

export default App;
