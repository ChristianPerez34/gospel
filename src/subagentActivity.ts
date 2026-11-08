import type {
  CurrentTurn,
  Message,
  SubagentActivity,
  SubagentKind,
  SubagentStep,
  VerificationActivity,
  VerificationOutcome,
} from "./types";

/** Folds one live sub-agent step into the activity shown on its parent row. */
export function applySubagentStep(
  activity: SubagentActivity | undefined,
  agent: SubagentKind,
  step: SubagentStep
): SubagentActivity {
  const base: SubagentActivity = activity ?? { agent, steps: [], text: "" };
  switch (step.type) {
    case "toolCall":
      if (base.steps.some((existing) => existing.id === step.id)) return base;
      return {
        ...base,
        latest: "tool",
        steps: [
          ...base.steps,
          { id: step.id, name: step.name, arguments: step.arguments, status: "calling" },
        ],
      };
    case "toolResult": {
      const index = base.steps.findIndex((existing) => existing.id === step.id);
      if (index < 0) {
        return {
          ...base,
          latest: "tool",
          steps: [
            ...base.steps,
            { id: step.id, name: step.name, result: step.result, status: "completed" },
          ],
        };
      }
      const steps = [...base.steps];
      steps[index] = { ...steps[index], result: step.result, status: "completed" };
      return { ...base, latest: "tool", steps };
    }
    case "text":
      return { ...base, latest: "text", text: base.text + step.text };
    case "textReset":
      return { ...base, text: step.text };
    case "phase":
      return { ...base, latest: "phase", phase: step.label };
  }
}

/** Applies a step to the tool block it belongs to. Steps for a parent the
 * turn does not hold are dropped rather than creating a placeholder row. */
export function applySubagentStepToTurn(
  turn: CurrentTurn,
  parentId: string,
  agent: SubagentKind,
  step: SubagentStep
): CurrentTurn {
  let found = false;
  const blocks = turn.blocks.map((block) => {
    if (block.kind !== "tool" || block.id !== parentId) return block;
    found = true;
    return { ...block, subagent: applySubagentStep(block.subagent, agent, step) };
  });
  return found ? { ...turn, blocks } : turn;
}

export function applyVerificationStep(
  verification: VerificationActivity | undefined,
  step: SubagentStep
): VerificationActivity {
  const base: VerificationActivity = verification ?? {
    status: "running",
    activity: { agent: "verification", steps: [], text: "" },
    concerns: [],
  };
  if (base.status !== "running") return base;
  return { ...base, activity: applySubagentStep(base.activity, "verification", step) };
}

export function completeVerification(
  verification: VerificationActivity | undefined,
  outcome: { status: VerificationOutcome; summary?: string; concerns?: string[] }
): VerificationActivity {
  const activity = verification?.activity ?? { agent: "verification", steps: [], text: "" };
  return {
    activity: {
      ...activity,
      // A verdict settles every step, including calls cut off by a timeout.
      steps: activity.steps.map((step) => ({ ...step, status: "completed" })),
    },
    status: outcome.status,
    summary: outcome.summary?.trim() || undefined,
    concerns: outcome.concerns ?? [],
  };
}

export function updateMessageById(
  messages: Message[],
  messageId: string,
  update: (message: Message) => Message
): Message[] {
  let changed = false;
  const next = messages.map((message) => {
    if (message.id !== messageId) return message;
    changed = true;
    return update(message);
  });
  return changed ? next : messages;
}
