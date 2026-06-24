export type AgentStatus = "idle" | "thinking" | "acting" | "error" | "connected";

export type MessageRole = "user" | "agent";

export type ActionCardType = "file" | "terminal" | "diff" | "search";

export type ThemePreference = "dark" | "light" | "system";

export type ResolvedTheme = "dark" | "light";

export type Severity = "Critical" | "High" | "Medium" | "Low" | "Info";

export type SignalTier = "tier_1" | "tier_2" | "noise" | "unclassified";

export type ReviewOutcome = "accepted" | "rejected";

export type ReviewMode =
  | { type: "local" }
  | { type: "pull_request"; pr_number: number }
  | { type: "full_scan" };

export interface ReviewComment {
  comment_id: string;
  file: string;
  line_start: number;
  line_end: number;
  severity: Severity;
  category: string;
  cwe_id?: string | null;
  cwe_name?: string | null;
  title: string;
  description: string;
  rationale?: string | null;
  evidence: string;
  suggestion?: string | null;
  verification_plan?: string | null;
  signal_tier: SignalTier;
}

export interface ReviewResult {
  run_id: string;
  comments: ReviewComment[];
  summary: string;
  validated: boolean;
  warnings: string[];
  files_scanned: number;
  mode: ReviewMode;
  suppressed_count: number;
  snr_percent: number;
  user_visible: boolean;
}

export interface ReviewOutcomeOutput {
  success: boolean;
  message: string;
  run_id: string;
  comment_id: string;
  outcome: ReviewOutcome;
  recorded_at: string;
}

export interface ToolCallActivity {
  id: string;
  name: string;
  arguments?: unknown;
  result?: string;
  status: "calling" | "completed";
}

export interface CurrentTurn {
  id: string;
  content: string;
  toolActivities: ToolCallActivity[];
}

export interface FinalizedToolActivity {
  messageId: string;
  activities: ToolCallActivity[];
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
  error?: string;
}

export interface ActionCard {
  id: string;
  type: ActionCardType;
  summary: string;
  detail?: string;
  sections?: ActionCardSection[];
  rawPayload?: string;
  expanded?: boolean;
  status?: "calling" | "completed";
}

export interface ActionCardField {
  label: string;
  value: string;
}

export interface ActionCardRow {
  primary: string;
  secondary?: string;
  meta?: string;
}

export type ActionCardSection =
  | {
      type: "fields";
      title?: string;
      fields: ActionCardField[];
    }
  | {
      type: "rows";
      title?: string;
      rows: ActionCardRow[];
      emptyText?: string;
    }
  | {
      type: "text";
      title?: string;
      content: string;
      monospace?: boolean;
    };

export interface Session {
  id: string;
  title: string;
  provider: string;
  model: string;
  timestamp: Date;
  messages: Message[];
  status: "idle" | "active" | "error";
  workspaceId?: string;
  backendCreated?: boolean;
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

export function modelOptionId(provider: string, model: string): string {
  return `${provider.toLowerCase()}::${model}`;
}
