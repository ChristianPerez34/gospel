import type { ReviewComment, ReviewMode, ReviewResult, SignalTier } from "./types";

interface ReviewFindingPromptContext {
  comment: ReviewComment;
  review?: ReviewResult | null;
  index?: number;
  workspacePath?: string;
}

const TIER_LABELS: Record<SignalTier, string> = {
  tier_1: "Tier 1",
  tier_2: "Tier 2",
  noise: "Noise",
  unclassified: "Unclassified",
};

export function isActionableReviewFinding(comment: ReviewComment) {
  return comment.signal_tier === "tier_1" || comment.signal_tier === "tier_2";
}

export function reviewModeLabel(mode: ReviewMode | undefined) {
  if (!mode) return undefined;
  if (mode.type === "local") return "local";
  if (mode.type === "pull_request") return `pr #${mode.pr_number}`;
  if (mode.type === "full_scan") return "full scan";
  return undefined;
}

function optionalLine(label: string, value: unknown) {
  if (value == null || value === "") return null;
  return `- ${label}: ${value}`;
}

function cweLabel(comment: ReviewComment) {
  if (comment.cwe_id && comment.cwe_name) return `${comment.cwe_id} (${comment.cwe_name})`;
  return comment.cwe_id ?? comment.cwe_name ?? "not specified";
}

function findingMetadata({
  comment,
  review,
  index,
  workspacePath,
}: ReviewFindingPromptContext) {
  return [
    optionalLine("Finding index", index),
    optionalLine("Finding ID", comment.comment_id),
    optionalLine("Tier", TIER_LABELS[comment.signal_tier]),
    optionalLine("Severity", comment.severity),
    optionalLine("Category", comment.category),
    optionalLine("CWE", cweLabel(comment)),
    optionalLine("File", `${comment.file}:${comment.line_start}-${comment.line_end}`),
    optionalLine("Review run", review?.run_id),
    optionalLine("Review mode", reviewModeLabel(review?.mode)),
    optionalLine("Workspace path", workspacePath),
  ].filter((line): line is string => Boolean(line));
}

export function buildExternalAgentFindingPrompt(context: ReviewFindingPromptContext) {
  const { comment } = context;
  const sections = [
    "You are an external coding agent. Work from this single security review finding and keep the response scoped to it.",
    ["Finding metadata:", ...findingMetadata(context)].join("\n"),
    `Title:\n${comment.title}`,
    `Description:\n${comment.description}`,
    comment.rationale ? `Rationale:\n${comment.rationale}` : null,
    `Evidence:\n${comment.evidence}`,
    comment.suggestion ? `Suggested fix:\n${comment.suggestion}` : null,
    comment.verification_plan ? `Verification plan:\n${comment.verification_plan}` : null,
    [
      "Scope:",
      "- Address only this finding.",
      "- Do not use review-run aggregate metrics as evidence.",
      "- Avoid unrelated refactors.",
    ].join("\n"),
  ];

  return sections.filter((section): section is string => Boolean(section)).join("\n\n");
}

export function buildGospelFixFindingPrompt(context: ReviewFindingPromptContext) {
  const { comment, review } = context;
  const rerunInstruction = review?.mode.type === "local"
    ? "- After the minimal fix, rerun `run_security_review` in `local` mode if the changed scope is still local."
    : review?.mode.type === "pull_request"
      ? `- After the minimal fix, rerun \`run_security_review\` in \`pr\` mode for PR #${review.mode.pr_number}.`
      : null;

  const workflow = [
    "Workflow:",
    "- Read the finding file first.",
    "- Inspect supporting files only when needed for the minimal safe fix.",
    "- Prefer the Source Edit Tool for narrow exact replacements.",
    "- Do not call `record_review_outcome` or mark this finding accepted/rejected.",
    "- Keep the final response concise and include verification performed.",
    rerunInstruction,
  ].filter((line): line is string => Boolean(line));

  const sections = [
    `Start with file: ${comment.file}`,
    "You are the main Gospel agent fixing one actionable security review finding in the active workspace.",
    ["Review metadata:", ...findingMetadata(context)].join("\n"),
    `Title:\n${comment.title}`,
    `Description:\n${comment.description}`,
    comment.rationale ? `Rationale:\n${comment.rationale}` : null,
    `Evidence:\n${comment.evidence}`,
    comment.suggestion ? `Suggested fix:\n${comment.suggestion}` : null,
    comment.verification_plan ? `Verification plan:\n${comment.verification_plan}` : null,
    workflow.join("\n"),
  ];

  return sections.filter((section): section is string => Boolean(section)).join("\n\n");
}
