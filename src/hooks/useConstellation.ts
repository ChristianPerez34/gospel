import { useMemo } from "react";
import type {
  CurrentTurn,
  Message,
  ReviewActivityEntry,
  ReviewFocus,
  TurnBlock,
} from "../types";
import type { UseReviewProgressState } from "./useReviewProgress";
import { FOCUS_OPTIONS, focusLabel } from "../utils/focus";

// ── Canvas node model ──────────────────────────────────────────────────────

export type CanvasToolKind =
  | "read"
  | "edit"
  | "write"
  | "run_shell"
  | "grep"
  | "search"
  | "plan"
  | "review"
  | "other";

export type CanvasToolStatus = "running" | "done" | "awaiting" | "error";

export interface CanvasToolNode {
  id: string;
  kind: CanvasToolKind;
  label: string;
  target: string;
  status: CanvasToolStatus;
  hasDiff: boolean;
  /** Raw arguments for diff popover extraction. */
  arguments?: unknown;
  /** Raw result for diff popover extraction. */
  result?: string;
}

export type CanvasReviewerStatus = "idle" | "active" | "done" | "failed";

export interface CanvasReviewerNode {
  id: string;
  focus: ReviewFocus;
  name: string;
  status: CanvasReviewerStatus;
  progress: number;
  findings: number;
  suppressed: number;
  comments: ReviewActivityEntry[];
}

export interface UseConstellationResult {
  toolNodes: CanvasToolNode[];
  reviewerNodes: CanvasReviewerNode[];
  reviewActive: boolean;
  agentRunning: boolean;
}

// ── Tool name → kind mapping ───────────────────────────────────────────────

const TOOL_KIND_MAP: Record<string, CanvasToolKind> = {
  read_file: "read",
  search_code: "grep",
  find_files: "search",
  list_directory: "search",
  delegate_exploration: "search",
  corpus_summary: "read",
  corpus_query: "grep",
  corpus_neighbors: "grep",
  write_harness_file: "plan",
  run_review: "review",
  run_security_review: "review",
  record_review_outcome: "review",
  source_edit: "edit",
  bash: "run_shell",
  terminal: "run_shell",
  apply_patch: "edit",
  write_file: "write",
  edit_file: "edit",
  replace_in_file: "edit",
};

const DIFF_TOOLS = new Set([
  "source_edit",
  "edit_file",
  "replace_in_file",
  "apply_patch",
  "write_file",
]);

const TOOL_LABELS: Record<string, string> = {
  read_file: "Read",
  search_code: "Search",
  find_files: "Find",
  list_directory: "List",
  delegate_exploration: "Explore",
  corpus_summary: "Corpus",
  corpus_query: "Query",
  corpus_neighbors: "Neighbors",
  write_harness_file: "Plan",
  run_review: "Review",
  run_security_review: "SecReview",
  record_review_outcome: "Record",
  source_edit: "Edit",
  bash: "Shell",
  terminal: "Shell",
  apply_patch: "Patch",
  write_file: "Write",
  edit_file: "Edit",
  replace_in_file: "Edit",
};

function mapToolKind(name: string): CanvasToolKind {
  return TOOL_KIND_MAP[name] ?? "other";
}

function mapToolStatus(block: Extract<TurnBlock, { kind: "tool" }>): CanvasToolStatus {
  if (block.status === "calling") return "running";
  return "done";
}

function extractTarget(name: string, args: unknown): string {
  if (!args || typeof args !== "object") return name;
  const record = args as Record<string, unknown>;
  const candidate =
    record.path ??
    record.pattern ??
    record.glob ??
    record.identifier ??
    record.command ??
    record.query ??
    record.file ??
    record.target_file;
  if (typeof candidate === "string" && candidate.length > 0) return candidate;
  return name;
}

// ── Review progress → reviewer nodes ────────────────────────────────────────

function reviewerStatusFromPipeline(
  pipeline: UseReviewProgressState["pipeline"]
): CanvasReviewerStatus {
  if (pipeline.done) return "done";
  if (pipeline.failed) return "failed";
  if (pipeline.detector.status === "active" || pipeline.validator === "active" || pipeline.finalize === "active")
    return "active";
  return "idle";
}

function reviewerProgressFromPipeline(
  pipeline: UseReviewProgressState["pipeline"]
): number {
  if (pipeline.done) return 1;
  if (pipeline.failed) return 1;
  const det = pipeline.detector;
  if (det.totalChunks > 0) {
    const chunkFrac = det.chunk / det.totalChunks;
    if (pipeline.finalize === "active") return 0.9;
    if (pipeline.validator === "active") return 0.7;
    return Math.min(0.6, chunkFrac * 0.6);
  }
  if (pipeline.detector.status === "active") return 0.2;
  return 0;
}

// ── Hook ───────────────────────────────────────────────────────────────────

export const CLUSTER_THRESHOLD = 12;
export const VISIBLE_TOOLS = 10;

export function useConstellation(
  messages: Message[],
  currentTurn: CurrentTurn | null,
  reviewProgress: UseReviewProgressState,
  isStreaming: boolean
): UseConstellationResult {
  return useMemo(() => {
    // Collect tool blocks from current turn + recent messages.
    const toolBlocks: Extract<TurnBlock, { kind: "tool" }>[] = [];
    const approvalBlocks: Extract<TurnBlock, { kind: "approval" }>[] = [];

    // Walk messages in order, then current turn, to build a chronological list.
    for (const msg of messages) {
      if (!msg.blocks) continue;
      for (const block of msg.blocks) {
        if (block.kind === "tool") toolBlocks.push(block);
        else if (block.kind === "approval") approvalBlocks.push(block);
      }
    }
    if (currentTurn?.blocks) {
      for (const block of currentTurn.blocks) {
        if (block.kind === "tool") toolBlocks.push(block);
        else if (block.kind === "approval") approvalBlocks.push(block);
      }
    }

    const toolNodes: CanvasToolNode[] = toolBlocks.map((block) => ({
      id: block.id,
      kind: mapToolKind(block.name),
      label: TOOL_LABELS[block.name] ?? block.name,
      target: extractTarget(block.name, block.arguments),
      status: mapToolStatus(block),
      hasDiff: DIFF_TOOLS.has(block.name),
      arguments: block.arguments,
      result: block.result,
    }));

    // Merge approval blocks as "awaiting" tool nodes (they represent pending
    // tool calls that need user approval before proceeding).
    for (const approval of approvalBlocks) {
      if (approval.status !== "pending") continue;
      toolNodes.push({
        id: approval.id,
        kind: approval.approvalKind === "command" ? "run_shell" : "edit",
        label: "Approve",
        target: approval.toolName,
        status: "awaiting",
        hasDiff: false,
      });
    }

    // Derive reviewer nodes from review progress perFocus.
    const reviewActive =
      reviewProgress.runId !== null &&
      !reviewProgress.done &&
      !reviewProgress.failed;
    const reviewerNodes: CanvasReviewerNode[] = FOCUS_OPTIONS.map((option) => {
      const fp = reviewProgress.perFocus[option.value];
      const pipeline = fp?.pipeline ?? reviewProgress.pipeline;
      const comments = reviewProgress.log.filter(
        (entry) => entry.focus === option.value
      );
      return {
        id: option.value,
        focus: option.value,
        name: focusLabel(option.value),
        status: fp ? reviewerStatusFromPipeline(pipeline) : "idle",
        progress: fp ? reviewerProgressFromPipeline(pipeline) : 0,
        findings: pipeline.findings,
        suppressed: pipeline.suppressed,
        comments,
      };
    }).filter((r) => r.status !== "idle" || reviewActive);

    const agentRunning = isStreaming;

    return {
      toolNodes,
      reviewerNodes,
      reviewActive: reviewActive || reviewProgress.done,
      agentRunning,
    };
  }, [messages, currentTurn, reviewProgress, isStreaming]);
}
