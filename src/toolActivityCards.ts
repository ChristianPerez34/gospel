import type {
  ActionCard,
  ActionCardField,
  ActionCardRow,
  ActionCardSection,
  ActionCardType,
  ReviewComment,
  ReviewResult,
  ToolCallActivity,
} from "./types";

const TOOL_LABELS: Record<string, string> = {
  read_file: "Read file",
  search_code: "Search code",
  find_files: "Find files",
  list_directory: "List directory",
  delegate_exploration: "Delegate exploration",
  corpus_summary: "Corpus summary",
  corpus_query: "Corpus query",
  corpus_neighbors: "Corpus neighbors",
  write_harness_file: "Update plan",
  run_security_review: "Security review",
  record_review_outcome: "Record review outcome",
  source_edit: "Edit file",
  bash: "Run command",
  terminal: "Run command",
  apply_patch: "Edit files",
  write_file: "Write file",
  edit_file: "Edit file",
  replace_in_file: "Edit file",
};

const MAX_PREVIEW_ROWS = 8;
const MAX_TEXT_PREVIEW_CHARS = 2400;

function startCase(value: string) {
  return value
    .replace(/_/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function formatValue(value: unknown) {
  if (value == null) return undefined;
  if (typeof value === "string") return value;

  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function parseJsonValue(value: unknown) {
  if (typeof value !== "string") return value;

  const trimmed = value.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return value;

  try {
    return JSON.parse(trimmed) as unknown;
  } catch {
    return value;
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }

  return value as Record<string, unknown>;
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function displayValue(value: unknown) {
  if (value == null || value === "") return undefined;
  if (typeof value === "boolean") return value ? "Yes" : "No";
  if (typeof value === "number") return value.toLocaleString();
  if (typeof value === "string") return value;
  return formatValue(value);
}

function field(label: string, value: unknown): ActionCardField | undefined {
  const display = displayValue(value);
  return display ? { label, value: display } : undefined;
}

function compactList(values: Array<unknown | undefined>) {
  return values
    .map(displayValue)
    .filter((value): value is string => Boolean(value))
    .join(", ");
}

function truncateText(value: string, limit = MAX_TEXT_PREVIEW_CHARS) {
  if (value.length <= limit) return value;
  return `${value.slice(0, limit).trimEnd()}\n[truncated for display]`;
}

function byteSize(value: unknown) {
  if (typeof value !== "number") return undefined;
  if (value < 1024) return `${value.toLocaleString()} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function rowsSection(
  title: string,
  rows: ActionCardRow[],
  emptyText?: string
): ActionCardSection {
  return { type: "rows", title, rows: rows.slice(0, MAX_PREVIEW_ROWS), emptyText };
}

function fieldsSection(title: string, fields: Array<ActionCardField | undefined>) {
  return {
    type: "fields" as const,
    title,
    fields: fields.filter((item): item is ActionCardField => Boolean(item)),
  };
}

function isRenderableSection(
  section: ActionCardSection | undefined
): section is ActionCardSection {
  if (!section) return false;
  return section.type !== "fields" || section.fields.length > 0;
}

function textSection(
  title: string,
  content: unknown,
  monospace = false
): ActionCardSection | undefined {
  const display = displayValue(content);
  return display
    ? { type: "text", title, content: truncateText(display), monospace }
    : undefined;
}

function rawPayload(activity: ToolCallActivity) {
  const sections: string[] = [];
  const argumentsText = formatValue(redactedArgumentsForDisplay(activity));
  const resultText = formatValue(activity.result);

  if (argumentsText) sections.push(`Arguments\n${argumentsText}`);
  if (resultText) sections.push(`Result\n${resultText}`);

  return sections.length > 0 ? sections.join("\n\n") : undefined;
}

function redactedArgumentsForDisplay(activity: ToolCallActivity) {
  if (activity.name !== "source_edit") return activity.arguments;
  const args = asRecord(parseJsonValue(activity.arguments));
  if (!args) return activity.arguments;
  return {
    ...args,
    old_text: args.old_text == null ? args.old_text : "[REDACTED]",
    new_text: args.new_text == null ? args.new_text : "[REDACTED]",
  };
}

function parsedArguments(activity: ToolCallActivity) {
  return asRecord(parseJsonValue(activity.arguments));
}

function parsedResult(activity: ToolCallActivity) {
  return parseJsonValue(activity.result);
}

function resultRecord(activity: ToolCallActivity) {
  return asRecord(parsedResult(activity));
}

function waitingSection(activity: ToolCallActivity): ActionCardSection[] {
  if (activity.status !== "calling") return [];
  return [{ type: "text", title: "Result", content: "Waiting for tool result..." }];
}

function failureSection(result: Record<string, unknown> | undefined) {
  if (!result || result.success !== false) return [];

  return [
    fieldsSection("Failure", [field("Reason", result.reason)]),
    textSection("Message", result.message) ?? {
      type: "text" as const,
      title: "Message",
      content: "Tool returned an unsuccessful result.",
    },
  ];
}

function readFileCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const path = result?.path ?? args?.path;
  const startLine = result?.start_line ?? args?.start_line;
  const endLine = result?.end_line ?? args?.end_line;
  const range = compactList([
    startLine ? `from ${startLine}` : undefined,
    endLine ? `to ${endLine}` : undefined,
  ]);
  const sections = [
    fieldsSection("Read", [
      field("Path", path),
      field("Lines", range || undefined),
      field("Total", result?.total_lines),
      field("Size", byteSize(result?.size_bytes)),
      field("Truncated", result?.truncated),
    ]),
    ...failureSection(result),
    textSection("Content", result?.content, true),
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([path, range]),
    sections,
  };
}

function searchCodeCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const matches = asArray(result?.matches);
  const rows = matches.map((match) => {
    const item = asRecord(match) ?? {};
    return {
      primary: displayValue(item.path) ?? "Unknown path",
      secondary: displayValue(item.text),
      meta: item.line ? `line ${item.line}` : undefined,
    };
  });
  const sections = [
    fieldsSection("Search", [
      field("Pattern", args?.pattern),
      field("Path", args?.path ?? "workspace"),
      field("Include", args?.include_glob),
      result ? field("Matches", matches.length) : undefined,
      result ? field("Scanned", result.scanned_files) : undefined,
      result ? field("Skipped", result.skipped_files) : undefined,
      result ? field("Truncated", result.truncated) : undefined,
    ]),
    ...failureSection(result),
    result ? rowsSection("Matches", rows, "No matches returned.") : undefined,
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([args?.pattern, args?.path, args?.include_glob]),
    sections,
  };
}

function findFilesCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const files = asArray(result?.files).map(displayValue).filter(Boolean) as string[];
  const sections = [
    fieldsSection("Find", [
      field("Glob", args?.glob),
      field("Path", args?.path ?? "workspace"),
      result ? field("Files", files.length) : undefined,
      result ? field("Scanned", result.scanned_entries) : undefined,
      result ? field("Truncated", result.truncated) : undefined,
    ]),
    ...failureSection(result),
    result
      ? rowsSection(
          "Files",
          files.map((filePath) => ({ primary: filePath })),
          "No files returned."
        )
      : undefined,
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([args?.glob, args?.path]),
    sections,
  };
}

function listDirectoryCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const entries = asArray(result?.entries);
  const rows = entries.map((entry) => {
    const item = asRecord(entry) ?? {};
    return {
      primary: displayValue(item.path) ?? displayValue(item.name) ?? "Unknown entry",
      secondary: compactList([item.kind, byteSize(item.size_bytes)]),
      meta: displayValue(item.kind),
    };
  });
  const sections = [
    fieldsSection("Directory", [
      field("Path", args?.path ?? "workspace"),
      field("Depth", args?.depth),
      result ? field("Entries", entries.length) : undefined,
      result ? field("Visited", result.visited_entries) : undefined,
      result ? field("Truncated", result.truncated) : undefined,
    ]),
    ...failureSection(result),
    result ? rowsSection("Entries", rows, "No entries returned.") : undefined,
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([args?.path ?? "workspace", args?.depth ? `depth ${args.depth}` : undefined]),
    sections,
  };
}

function delegateExplorationCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const toolsUsed = asArray(result?.tools_used).map(displayValue).filter(Boolean) as string[];
  const keyFiles = asArray(result?.key_files).map(displayValue).filter(Boolean) as string[];
  const findings = asArray(result?.findings).map(displayValue).filter(Boolean) as string[];
  const constraints = asArray(result?.constraints).map(displayValue).filter(Boolean) as string[];
  const nextReads = asArray(result?.suggested_next_reads).map(displayValue).filter(Boolean) as string[];

  const sections = [
    fieldsSection("Investigation", [
      result ? field("Success", result.success) : undefined,
      result ? field("Summary lines", result.summary ? 1 : 0) : undefined,
      result ? field("Key files", keyFiles.length || undefined) : undefined,
      result ? field("Findings", findings.length || undefined) : undefined,
      result ? field("Constraints", constraints.length || undefined) : undefined,
      result ? field("Next reads", nextReads.length || undefined) : undefined,
      result ? field("Tools", toolsUsed.length || undefined) : undefined,
      result ? field("Truncated", result.truncated) : undefined,
      result ? field("Reason", result.reason) : undefined,
    ]),
    textSection("Task", args?.task),
    textSection("Summary", result?.summary),
    textSection("Message", result?.message),
    textSection("Report", result?.report, true),
    result
      ? rowsSection(
          "Key files",
          keyFiles.map((file) => ({ primary: file })),
          "No key files returned."
        )
      : undefined,
    result
      ? rowsSection(
          "Findings",
          findings.map((finding) => ({ primary: finding })),
          "No findings returned."
        )
      : undefined,
    result
      ? rowsSection(
          "Constraints",
          constraints.map((constraint) => ({ primary: constraint })),
          "No constraints returned."
        )
      : undefined,
    result
      ? rowsSection(
          "Suggested next reads",
          nextReads.map((item) => ({ primary: item })),
          "No recommended next reads."
        )
      : undefined,
    result
      ? rowsSection(
          "Tools used",
          toolsUsed.map((tool) => ({ primary: tool })),
          "No tools reported."
        )
      : undefined,
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([result?.summary, args?.task]),
    sections,
  };
}

function corpusSummaryCard(activity: ToolCallActivity): Partial<ActionCard> {
  const result = resultRecord(activity);
  const topSymbols = asArray(result?.top_symbols).map((symbol) => {
    const tuple = asArray(symbol);
    return {
      primary: displayValue(tuple[0]) ?? "Unknown symbol",
      meta: tuple[1] ? `${tuple[1]} refs` : undefined,
    };
  });
  const sections = [
    fieldsSection("Corpus", [
      field("Exists", result?.exists),
      field("Files", result?.file_count),
      field("Symbols", result?.symbol_count),
      field("Relations", result?.relationship_count),
    ]),
    textSection("Message", result?.message),
    result ? rowsSection("Top symbols", topSymbols, "No top symbols returned.") : undefined,
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return { sections };
}

function corpusQueryCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const node = asRecord(result?.node);
  const sections = [
    fieldsSection("Query", [
      field("Identifier", args?.identifier),
      result ? field("Found", result.found) : undefined,
      result ? field("Neighbors", result.neighbor_count) : undefined,
    ]),
    node
      ? fieldsSection("Node", [
          field("Name", node.name),
          field("Kind", node.kind),
          field("Type", node.node_type),
          field("ID", node.id),
        ])
      : undefined,
    textSection("Message", result?.message),
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: displayValue(args?.identifier),
    sections,
  };
}

function corpusNeighborsCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = parsedResult(activity);
  const hasResult = activity.result != null;
  const hasNeighborList = Array.isArray(result);
  const neighbors = hasNeighborList ? result : [];
  const rows = neighbors.map((neighbor) => {
    const item = asRecord(neighbor) ?? {};
    return {
      primary: displayValue(item.node_name) ?? displayValue(item.node_id) ?? "Unknown node",
      secondary: compactList([item.relationship_type, item.direction, item.node_kind]),
      meta: displayValue(item.confidence),
    };
  });
  const sections = [
    fieldsSection("Neighbors", [
      field("Identifier", args?.identifier),
      field("Confidence", args?.min_confidence ?? "low"),
      hasNeighborList ? field("Count", neighbors.length) : undefined,
    ]),
    hasNeighborList ? rowsSection("Relationships", rows, "No relationships returned.") : undefined,
    hasResult && !hasNeighborList ? textSection("Result", result, true) : undefined,
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: displayValue(args?.identifier),
    sections,
  };
}

function writeHarnessFileCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const path = result?.path ?? args?.path;
  const sections = [
    fieldsSection("Harness file", [
      field("Path", path),
      result ? field("Size", byteSize(result.size_bytes)) : undefined,
    ]),
    ...failureSection(result),
    textSection("Content", args?.content, true),
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: displayValue(path),
    sections,
  };
}

function sourceEditCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const path = result?.path ?? args?.path;
  const startLine = result?.start_line;
  const endLine = result?.end_line;
  const range = compactList([
    startLine ? `from ${startLine}` : undefined,
    endLine ? `to ${endLine}` : undefined,
  ]);
  const sections = [
    fieldsSection("Edit", [
      field("Path", path),
      result ? field("Changed", result.changed) : undefined,
      result ? field("Replacements", result.replacements) : undefined,
      result ? field("Lines", range || undefined) : undefined,
      result ? field("Size", byteSize(result.size_bytes)) : undefined,
      result ? field("Truncated", result.truncated) : undefined,
    ]),
    ...failureSection(result),
    textSection("Diff", result?.diff_preview, true),
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([path, range]),
    sections,
  };
}

function reviewModeLabel(mode: unknown) {
  const record = asRecord(mode);
  const type = displayValue(record?.type);
  if (type === "pull_request") {
    return record?.pr_number ? `PR #${record.pr_number}` : "PR";
  }
  if (type === "full_scan") return "Full scan";
  if (type === "local") return "Local";
  return displayValue(mode);
}

function severityBadge(comment: Partial<ReviewComment>) {
  return compactList([comment.severity, comment.signal_tier, comment.category]);
}

function runSecurityReviewCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const review = asRecord(result?.review) as ReviewResult | undefined;
  const findings = asArray(result?.findings);
  const comments = review?.comments ?? [];
  const rows = findings.length > 0
    ? findings.map((finding, fallbackIndex) => {
        const item = asRecord(finding) ?? {};
        const index = displayValue(item.index) ?? String(fallbackIndex + 1);
        return {
          primary: `[${index}] ${displayValue(item.title) ?? "Untitled finding"}`,
          secondary: compactList([
            item.file,
            item.line_start ? `line ${item.line_start}` : undefined,
            severityBadge(item as Partial<ReviewComment>),
          ]),
          meta: displayValue(item.comment_id),
        };
      })
    : comments.map((comment, index) => ({
        primary: `[${index + 1}] ${comment.title}`,
        secondary: compactList([
          comment.file,
          `line ${comment.line_start}`,
          severityBadge(comment),
        ]),
        meta: comment.comment_id,
      }));

  const totalFindings = review
    ? review.comments.length + review.suppressed_count
    : rows.length || undefined;
  const sections = [
    fieldsSection("Review", [
      field("Mode", reviewModeLabel(review?.mode) ?? args?.mode),
      field("Run", review?.run_id),
      field("Findings", totalFindings),
      field("Shown", review?.comments.length),
      field("Suppressed", review?.suppressed_count),
      field("SNR", review ? `${review.snr_percent}%` : undefined),
      field("Files", review?.files_scanned),
      field("Visible", review?.user_visible),
    ]),
    ...failureSection(result),
    textSection("Summary", result?.message ?? review?.summary),
    rowsSection("Findings", rows, "No findings returned."),
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([args?.mode, review?.run_id]),
    sections,
  };
}

function recordReviewOutcomeCard(activity: ToolCallActivity): Partial<ActionCard> {
  const args = parsedArguments(activity);
  const result = resultRecord(activity);
  const sections = [
    fieldsSection("Outcome", [
      field("Run", result?.run_id ?? args?.run_id),
      field("Comment", result?.comment_id ?? args?.comment_id),
      field("Outcome", result?.outcome ?? args?.outcome),
      field("Success", result?.success),
      field("Reason", result?.reason),
    ]),
    textSection("Message", result?.message),
    ...waitingSection(activity),
  ].filter(isRenderableSection);

  return {
    detail: compactList([args?.outcome, args?.comment_id]),
    sections,
  };
}

function fallbackCard(activity: ToolCallActivity): Partial<ActionCard> {
  const sections = [
    textSection("Arguments", formatValue(activity.arguments), true),
    textSection("Result", formatValue(activity.result), true),
    ...waitingSection(activity),
  ].filter((section): section is ActionCardSection => Boolean(section));

  return { sections };
}

function actionCardTypeForTool(name: string): ActionCardType {
  const normalized = name.toLowerCase();

  if (
    normalized.includes("search") ||
    normalized.includes("find") ||
    normalized.includes("exploration") ||
    normalized.includes("review")
  ) {
    return "search";
  }

  if (
    normalized.includes("bash") ||
    normalized.includes("command") ||
    normalized.includes("terminal")
  ) {
    return "terminal";
  }

  if (
    normalized.includes("patch") ||
    normalized.includes("edit") ||
    normalized.includes("write") ||
    normalized.includes("replace")
  ) {
    return "diff";
  }

  return "file";
}

export function formatToolActivityLabel(
  activity: Pick<ToolCallActivity, "name" | "status">,
  live = false
) {
  const label = TOOL_LABELS[activity.name] ?? startCase(activity.name);
  return live && activity.status === "calling" ? `${label}...` : label;
}

export function summarizeLiveToolActivity(
  activities: ToolCallActivity[],
  isThinking: boolean
) {
  const activeActivity = [...activities]
    .reverse()
    .find((activity) => activity.status === "calling");

  if (activeActivity) {
    return formatToolActivityLabel(activeActivity, true);
  }

  if (activities.length > 0) {
    return "Finalizing response...";
  }

  return isThinking ? "Thinking..." : null;
}

export function toolActivitiesToActionCards(
  activities: ToolCallActivity[]
): ActionCard[] {
  return activities.map((activity) => {
    const formatter = TOOL_CARD_FORMATTERS[activity.name] ?? fallbackCard;
    const formatted = formatter(activity);

    return {
      id: activity.id,
      type: actionCardTypeForTool(activity.name),
      summary: formatToolActivityLabel(activity),
      rawPayload: rawPayload(activity),
      expanded: activity.status === "calling",
      status: activity.status,
      ...formatted,
    };
  });
}

const TOOL_CARD_FORMATTERS: Record<string, (activity: ToolCallActivity) => Partial<ActionCard>> = {
  read_file: readFileCard,
  search_code: searchCodeCard,
  find_files: findFilesCard,
  list_directory: listDirectoryCard,
  delegate_exploration: delegateExplorationCard,
  corpus_summary: corpusSummaryCard,
  corpus_query: corpusQueryCard,
  corpus_neighbors: corpusNeighborsCard,
  write_harness_file: writeHarnessFileCard,
  source_edit: sourceEditCard,
  run_security_review: runSecurityReviewCard,
  record_review_outcome: recordReviewOutcomeCard,
};
