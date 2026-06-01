import type { ActionCard, ActionCardType, ToolCallActivity } from "./types";

const TOOL_LABELS: Record<string, string> = {
  read_file: "Read file",
  search_code: "Search code",
  find_files: "Find files",
  list_directory: "List directory",
  delegate_exploration: "Delegate exploration",
  bash: "Run command",
  terminal: "Run command",
  apply_patch: "Edit files",
  write_file: "Write file",
  edit_file: "Edit file",
  replace_in_file: "Edit file",
};

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

function actionCardTypeForTool(name: string): ActionCardType {
  const normalized = name.toLowerCase();

  if (
    normalized.includes("search") ||
    normalized.includes("find") ||
    normalized.includes("exploration")
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

function formatToolActivityContent(activity: ToolCallActivity) {
  const sections: string[] = [];
  const argumentsText = formatValue(activity.arguments);
  const resultText = formatValue(activity.result);

  if (argumentsText) {
    sections.push(`Arguments\n${argumentsText}`);
  }

  if (resultText) {
    sections.push(`Result\n${resultText}`);
  }

  return sections.length > 0 ? sections.join("\n\n") : undefined;
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
  return activities.map((activity) => ({
    id: activity.id,
    type: actionCardTypeForTool(activity.name),
    summary: formatToolActivityLabel(activity),
    content: formatToolActivityContent(activity),
    expanded: activity.status === "calling",
    status: activity.status,
  }));
}
