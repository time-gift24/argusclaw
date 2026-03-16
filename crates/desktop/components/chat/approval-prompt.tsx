"use client";

import { chat } from "@/lib/tauri";
import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

export function ApprovalPrompt() {
  const session = useActiveChatSession();
  const pendingRequest = session?.pendingApprovalRequest;

  if (!pendingRequest) return null;

  const handleApprove = () => {
    void chat.resolveApproval(
      session!.runtimeAgentId,
      pendingRequest.id,
      "approved",
      "desktop-user",
    );
  };

  const handleDeny = () => {
    void chat.resolveApproval(
      session!.runtimeAgentId,
      pendingRequest.id,
      "denied",
      "desktop-user",
    );
  };

  return (
    <Card className="mx-4 mb-2 border-orange-200 bg-orange-50 dark:border-orange-800 dark:bg-orange-950">
      <CardHeader className="pb-2">
        <CardTitle className="text-base">需要审批: {pendingRequest.tool_name}</CardTitle>
      </CardHeader>
      <CardContent className="pb-2">
        <p className="mb-2 text-sm text-muted-foreground">
          风险等级: {pendingRequest.risk_level}
        </p>
        <pre className="overflow-auto rounded bg-muted p-2 text-xs">
          {pendingRequest.action}
        </pre>
      </CardContent>
      <CardFooter className="gap-2">
        <Button size="sm" onClick={handleApprove}>
          批准
        </Button>
        <Button size="sm" variant="outline" onClick={handleDeny}>
          拒绝
        </Button>
      </CardFooter>
    </Card>
  );
}
