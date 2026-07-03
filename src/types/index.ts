export type AgentStatus = "idle" | "thinking" | "acting" | "error" | "connected";

export type MessageRole = "user" | "agent";

export type ActionCardType = "file" | "terminal" | "diff" | "search";

export type ThemePreference = "dark" | "light" | "system";

export type ResolvedTheme = "dark" | "light";

export type SessionMode = "Build" | "ReadOnly";

export type Severity = "Critical" | "High" | "Medium" | "Low" | "Info";

export type SignalTier = "tier_1" | "tier_2" | "noise" | "unclassified";

export type ReviewOutcome = "accepted" | "rejected";

export type ReviewFocus =
  | "Security"
  | "BugHunt"
  | "Architecture"
  | "Performance"
  | "Style";

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
  focus: ReviewFocus;
  focus_subcategory?: string | null;
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
  focus: ReviewFocus;
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

// ── Real-time review progress (review-progress event) ──

export interface ChunkFailure {
  kind: string;
  detail: string;
}

export interface PhaseFailure {
  detail: string;
}

export type ChunkStatus =
  | "starting"
  | "running"
  | "done"
  | { failed: ChunkFailure };

export type PhaseStatus = "running" | "done" | { failed: PhaseFailure };

export type ReviewPhase =
  | {
      type: "detector";
      chunk: number;
      totalChunks: number;
      files: string[];
      candidateCount: number;
      status: ChunkStatus;
    }
  | {
      type: "validator";
      candidateCount: number;
      status: PhaseStatus;
    }
  | {
      type: "detectorTool";
      chunk: number;
      toolName: string;
      event:
        | { call: { arguments: unknown } }
        | { result: { summary: string } };
    }
  | { type: "finalize"; status: PhaseStatus }
  | { type: "done"; findings: number; suppressed: number }
  | { type: "failed"; detail: string };

export interface ReviewProgressEvent {
  run_id: string;
  phase: ReviewPhase;
  timestamp: number;
}

/** Per-node state derived in the hook from the event stream. */
export type ReviewNodeState = "idle" | "active" | "done" | "failed";

export interface ReviewDetectorState {
  chunk: number;
  totalChunks: number;
  candidateCount: number;
  status: ReviewNodeState;
}

export interface ReviewPipelineState {
  detector: ReviewDetectorState;
  validator: ReviewNodeState;
  finalize: ReviewNodeState;
  /** Whole-run outcome once known. */
  done: boolean;
  failed: boolean;
  failureDetail: string | null;
  findings: number;
  suppressed: number;
}

export interface ReviewActivityEntry {
  timestamp: number;
  phase: ReviewPhase["type"];
  text: string;
}

export interface ToolCallActivity {
  id: string;
  name: string;
  arguments?: unknown;
  result?: string;
  status: "calling" | "completed";
}

export type TurnBlock =
  | { kind: "text"; id: string; text: string }
  | {
      kind: "tool";
      id: string;
      name: string;
      arguments?: unknown;
      result?: string;
      status: "calling" | "completed";
    };

export interface CurrentTurn {
  id: string;
  blocks: TurnBlock[];
  createdAt: Date;
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
  blocks?: TurnBlock[];
}

export interface ActionCard {
  id: string;
  type: ActionCardType;
  summary: string;
  detail?: string;
  /** Primary target (path / pattern / glob / identifier) shown inline in the
   * compact timeline header and used as the grouping key. */
  target?: string;
  sections?: ActionCardSection[];
  rawPayload?: string;
  expanded?: boolean;
  status?: "calling" | "completed";
  /** Number of consecutive identical calls merged into this step (>= 2 when grouped). */
  groupCount?: number;
  /** Per-call cards when consecutive identical calls are grouped into one step. */
  passes?: ActionCard[];
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
  mode?: SessionMode;
  timestamp: Date;
  messages: Message[];
  status: "idle" | "active" | "error" | "archived";
  workspaceId?: string;
  backendCreated?: boolean;
  archivedAt?: Date;
}

export interface ArchivePolicy {
  workspace_id: string | null;
  retention_days: number;
  auto_archive_hours: number;
  uses_workspace_override: boolean;
}

export interface ArchiveStats {
  workspace_id: string | null;
  live_count: number;
  archived_count: number;
  expired_count: number;
  archived_bytes: number;
  oldest_archived_at: string | null;
}

export type McpServerKind = "built_in" | "custom";

export type McpSafetyClass = "read_only" | "mutating" | "unknown";

export interface McpEnvValue {
  key: string;
  value: string;
}

export interface McpToolInventoryItem {
  name: string;
  description?: string | null;
}

export interface McpServer {
  id: string;
  kind: McpServerKind;
  displayName: string;
  description?: string | null;
  enabled: boolean;
  trusted: boolean;
  trustRevokedReason?: string | null;
  safetyClass: McpSafetyClass;
  scope: string;
  command?: string | null;
  args: string[];
  env: McpEnvValue[];
  secretEnvKeys: string[];
  readiness: string;
  health: string;
  inventory: McpToolInventoryItem[];
  lastErrorSummary?: string | null;
  lastSuccessAt?: string | null;
  lastResolvedExecutablePath?: string | null;
  externalFingerprint?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface CreateMcpServerRequest {
  displayName: string;
  command: string;
  args: string[];
  env: McpEnvValue[];
  secretEnvKeys: string[];
  safetyClass?: McpSafetyClass;
  scope?: string;
}

export type UpdateMcpServerRequest = CreateMcpServerRequest;

export interface McpImportFieldDiff {
  field: string;
  current?: string | null;
  incoming?: string | null;
}

export interface McpImportPreviewServer {
  externalId: string;
  name: string;
  proposed: CreateMcpServerRequest;
  matchedServerId?: string | null;
  conflict: boolean;
  fieldDiffs: McpImportFieldDiff[];
  warnings: string[];
}

export interface McpImportPreview {
  token: string;
  sourcePath: string;
  servers: McpImportPreviewServer[];
  warnings: string[];
}

export interface McpApplyImportResult {
  created: string[];
  updated: string[];
  skipped: string[];
  warnings: string[];
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

export function normalizeSessionMode(mode?: string | null): SessionMode {
  return mode === "ReadOnly" ? "ReadOnly" : "Build";
}
