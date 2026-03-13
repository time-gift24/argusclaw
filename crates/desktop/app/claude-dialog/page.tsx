"use client";

import { useState } from "react";
import {
  PieChart,
  Brain,
  Plus,
  Paperclip,
  Send,
  ChevronDown,
  Square,
} from "lucide-react";
import { cn } from "@/lib/utils";

// Types
type MessageRole = "user" | "assistant";

interface Message {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: Date;
}

// Mock data
const MOCK_MESSAGES: Message[] = [
  {
    id: "1",
    role: "assistant",
    content: "Hi! I'm Claude, an AI assistant created by Anthropic. How can I help you today?",
    timestamp: new Date(),
  },
];

const MOCK_TOOLS = [
  { id: "1", name: "Web Search", icon: PieChart },
  { id: "2", name: "Thinking", icon: Brain },
];

function ClaudeDialog() {
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<Message[]>(MOCK_MESSAGES);
  const [isRunning, setIsRunning] = useState(false);
  const [showTools, setShowTools] = useState(false);

  const handleSend = () => {
    if (!input.trim()) return;

    const userMessage: Message = {
      id: Date.now().toString(),
      role: "user",
      content: input,
      timestamp: new Date(),
    };
    setMessages((prev) => [...prev, userMessage]);
    setInput("");
    setIsRunning(true);

    setTimeout(() => {
      const assistantMessage: Message = {
        id: (Date.now() + 1).toString(),
        role: "assistant",
        content: "This is a mock response. In a real implementation, this would come from the LLM.",
        timestamp: new Date(),
      };
      setMessages((prev) => [...prev, assistantMessage]);
      setIsRunning(false);
    }, 1500);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="flex h-full flex-col bg-background">
      {/* Messages Area - 居中显示 */}
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto flex h-full max-w-2xl flex-col justify-end px-4 py-8">
          <div className="space-y-6">
            {messages.map((message) => (
              <div
                key={message.id}
                className={cn(
                  "flex gap-4",
                  message.role === "user" && "flex-row-reverse"
                )}
              >
                {/* Avatar */}
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-medium">
                  {message.role === "assistant" ? "C" : "Y"}
                </div>

                {/* Message Content */}
                <div
                  className={cn(
                    "max-w-[80%] rounded-lg px-4 py-2",
                    message.role === "user" ? "bg-muted" : "bg-transparent"
                  )}
                >
                  <p className="text-sm">{message.content}</p>
                </div>
              </div>
            ))}

            {isRunning && (
              <div className="flex gap-4">
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-medium">
                  C
                </div>
                <div className="flex items-center gap-1">
                  <div className="h-2 w-2 animate-bounce rounded-full bg-muted-foreground/40" />
                  <div className="h-2 w-2 animate-bounce rounded-full bg-muted-foreground/40 [animation-delay:0.15s]" />
                  <div className="h-2 w-2 animate-bounce rounded-full bg-muted-foreground/40 [animation-delay:0.3s]" />
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Input Area - 悬浮居中 */}
      <div className="relative px-4 pb-6 pt-2">
        <div className="mx-auto max-w-2xl">
          <div className="relative flex flex-col rounded-xl bg-background/80 backdrop-blur-sm shadow-lg">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="How can I help you today?"
              className="min-h-[60px] max-h-[200px] w-full resize-none bg-transparent px-4 py-4 text-sm outline-none placeholder:text-muted-foreground"
              rows={1}
            />

            {/* Action Bar */}
            <div className="flex items-center justify-between px-2 pb-2">
              {/* Left Actions */}
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  <Plus className="size-4" />
                </button>
                <button
                  type="button"
                  onClick={() => setShowTools(!showTools)}
                  className={cn(
                    "flex h-8 items-center gap-1 rounded-md px-2 text-muted-foreground hover:bg-muted hover:text-foreground",
                    showTools && "bg-muted text-foreground"
                  )}
                >
                  <Paperclip className="size-4" />
                  <span className="text-xs">Tools</span>
                  <ChevronDown className="size-3" />
                </button>
                <button
                  type="button"
                  className="flex h-8 items-center gap-1 rounded-md px-2 text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  <Brain className="size-4" />
                  <span className="text-xs">Thinking</span>
                </button>
              </div>

              {/* Right Actions */}
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  className="flex h-8 items-center gap-1 rounded-md px-2 text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  <span className="text-xs">Sonnet 4.5</span>
                  <ChevronDown className="size-3" />
                </button>
                {isRunning ? (
                  <button
                    type="button"
                    onClick={() => setIsRunning(false)}
                    className="flex h-8 w-8 items-center justify-center rounded-full bg-muted text-muted-foreground hover:bg-muted/80"
                  >
                    <Square className="size-3 fill-current" />
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={handleSend}
                    disabled={!input.trim()}
                    className="flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-90 disabled:opacity-50"
                  >
                    <Send className="size-4" />
                  </button>
                )}
              </div>
            </div>

            {/* Tools Menu */}
            {showTools && (
              <div className="border-t border-border/50 px-2 pb-2">
                <div className="flex gap-1 py-2">
                  {MOCK_TOOLS.map((tool) => (
                    <button
                      key={tool.id}
                      type="button"
                      className="flex items-center gap-1.5 rounded-md bg-muted/50 px-3 py-1.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
                    >
                      <tool.icon className="size-3.5" />
                      {tool.name}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>

          <p className="mt-2 text-center text-xs text-muted-foreground">
            Claude can make mistakes. Please use with discretion.
          </p>
        </div>
      </div>
    </div>
  );
}

export default function ClaudeDialogPage() {
  return (
    <div className="flex min-h-svh flex-col bg-background">
      {/* Desktop: 1024px */}
      <div className="hidden lg:flex min-h-[400px] items-center justify-center border-b border-border/10 p-4">
        <div className="w-full max-w-3xl h-[600px] border border-border/20 rounded-xl overflow-hidden shadow-sm">
          <ClaudeDialog />
        </div>
      </div>

      {/* Tablet: 768px */}
      <div className="hidden md:flex lg:hidden min-h-[400px] items-center justify-center border-b border-border/10 p-4">
        <div className="w-full max-w-xl h-[500px] border border-border/20 rounded-xl overflow-hidden shadow-sm">
          <ClaudeDialog />
        </div>
      </div>

      {/* Mobile: 375px */}
      <div className="flex md:hidden min-h-[400px] items-center justify-center p-2">
        <div className="w-full h-[550px] border border-border/20 rounded-xl overflow-hidden shadow-sm">
          <ClaudeDialog />
        </div>
      </div>

      {/* Resolution info */}
      <div className="p-4 text-center text-xs text-muted-foreground">
        <p>当前布局响应式展示 - 调整窗口宽度查看不同分辨率效果</p>
        <p className="mt-1">Desktop (≥1024px) | Tablet (768-1023px) | Mobile (&lt;768px)</p>
      </div>
    </div>
  );
}
