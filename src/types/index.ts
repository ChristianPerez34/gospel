export type AgentStatus = "idle" | "thinking" | "acting" | "error" | "connected";

export type MessageRole = "user" | "agent";

export type ActionCardType = "file" | "terminal" | "diff" | "search";

export interface ToolCallActivity {
  name: string;
  arguments?: unknown;
  result?: string;
  status: "calling" | "completed";
}

export interface User {
  name: string;
  avatar?: string;
}

export interface Message {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: Date;
  actionCards?: ActionCard[];
  error?: string;
}

export interface ActionCard {
  id: string;
  type: ActionCardType;
  summary: string;
  content?: string;
  expanded?: boolean;
}

export interface Session {
  id: string;
  title: string;
  provider: string;
  model: string;
  timestamp: Date;
  messages: Message[];
  status: "idle" | "active" | "error";
}

export interface Workspace {
  id: string;
  name: string;
  path: string;
  sessionCount: number;
}

export interface ModelOption {
  id: string;
  name: string;
  provider: string;
  configured?: boolean;
}

export interface ProviderStatus {
  provider: string;
  configured: boolean;
}
