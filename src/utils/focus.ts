import type { ReviewFocus } from "../types";

export interface FocusOption {
  value: ReviewFocus;
  label: string;
  className: string;
}

export const FOCUS_OPTIONS: Array<FocusOption> = [
  { value: "Security", label: "Security", className: "border-status-error text-status-error" },
  { value: "BugHunt", label: "Bug Hunt", className: "border-accent-data text-accent-data" },
  { value: "Architecture", label: "Architecture", className: "border-accent-structure text-accent-structure" },
  { value: "Performance", label: "Performance", className: "border-status-warning text-status-warning" },
  { value: "Style", label: "Style", className: "border-text-muted text-text-secondary" },
];

export function focusOption(focus: ReviewFocus): FocusOption | undefined {
  return FOCUS_OPTIONS.find((option) => option.value === focus);
}

export function focusLabel(focus: ReviewFocus): string {
  return focusOption(focus)?.label ?? focus;
}

export const FOCUS_ORDER: ReviewFocus[] = FOCUS_OPTIONS.map((option) => option.value);
