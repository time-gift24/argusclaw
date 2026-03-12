// crates/desktop/src/lib/chat-types.ts

/**
 * 聊天消息类型定义
 */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: Date;
}

/**
 * 消息角色类型
 */
export type MessageRole = "user" | "assistant";

/**
 * Mock 响应映射类型
 */
export type MockResponseKey = "default" | "code" | "mermaid" | "math";
