import type { ReviewFocus } from "../types";
import { focusLabel, focusOption } from "../utils/focus";

export function FocusBadge({ focus }: { focus: ReviewFocus }) {
  const className = focusOption(focus)?.className ?? "border-text-muted text-text-secondary";
  return (
    <span
      className={`inline-flex h-5 items-center rounded-sm border px-1.5 font-mono text-caption ${className}`}
    >
      {focusLabel(focus)}
    </span>
  );
}
