import { useState, useEffect, useRef } from "react";
import { Button } from "@/components/ui/button";
import type { ProviderConfig } from "./ProviderSelector";
import { ProviderSelector } from "./ProviderSelector";
import { McpSettingsPanel } from "./McpSettingsPanel";
import type { ArchivePolicy, ArchiveStats, ThemePreference } from "../types";
import { useFocusTrap } from "../hooks/useFocusTrap";

function themeIndex(value: ThemePreference) {
  switch (value) {
    case "dark":
      return 0;
    case "light":
      return 1;
    case "system":
      return 2;
  }
}

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
  isRefreshingModels: boolean;
  initialTab?: TabKey;
  themePreference: ThemePreference;
  onThemePreferenceChange: (theme: ThemePreference) => void;
  archivePolicy?: ArchivePolicy | null;
  workspaceArchivePolicy?: ArchivePolicy | null;
  archiveStats?: ArchiveStats | null;
  activeWorkspaceName?: string;
  onArchivePolicyChange?: (
    workspaceId: string | null,
    retentionDays: number,
    autoArchiveHours: number
  ) => Promise<void>;
  onClearWorkspaceArchivePolicy?: () => Promise<void>;
  onRunArchiveMaintenance?: () => Promise<void>;
  archivePolicySaving?: boolean;
}

type TabKey = "general" | "models" | "mcp" | "data";

const THEME_OPTIONS: Array<{ value: ThemePreference; label: string; detail: string }> = [
  { value: "dark", label: "Dark", detail: "Default focus theme" },
  { value: "light", label: "Light", detail: "Bright surfaces" },
  { value: "system", label: "System", detail: "Follow OS" },
];

const RETENTION_OPTIONS = [
  { value: 7, label: "7d" },
  { value: 30, label: "30d" },
  { value: 90, label: "90d" },
  { value: 365, label: "365d" },
];

const AUTO_ARCHIVE_OPTIONS = [
  { value: 0, label: "Off" },
  { value: 1, label: "1h" },
  { value: 24, label: "24h" },
  { value: 168, label: "7d" },
  { value: 720, label: "30d" },
];

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function SettingsModal({
  open,
  onClose,
  providers,
  onProvidersChange,
  onRefreshAvailability,
  isRefreshingModels,
  initialTab = "models",
  themePreference,
  onThemePreferenceChange,
  archivePolicy,
  workspaceArchivePolicy,
  archiveStats,
  activeWorkspaceName,
  onArchivePolicyChange,
  onClearWorkspaceArchivePolicy,
  onRunArchiveMaintenance,
  archivePolicySaving = false,
}: SettingsModalProps) {
  const [activeTab, setActiveTab] = useState<TabKey>(initialTab);
  const themeOptionRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const dialogRef = useRef<HTMLDivElement>(null);
  const activeThemeIndex = themeIndex(themePreference);

  useEffect(() => {
    if (!open) return;
    setActiveTab(initialTab);
  }, [initialTab, open]);

  useFocusTrap({
    active: open,
    containerRef: dialogRef,
    onEscape: onClose,
    shouldRestoreFocusOnDeactivate: true,
  });

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 bg-scrim z-[var(--z-dialog)] flex items-center justify-center animate-fade-in"
      onClick={onClose}
    >
      <div
        ref={dialogRef}
        className="w-[680px] max-w-[92vw] max-h-[84vh] flex flex-col bg-surface-elevated border border-surface-overlay rounded-lg shadow-[var(--shadow-dialog)] animate-slide-up"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-overlay shrink-0">
          <h2
            className="font-sans text-base font-semibold text-text-primary m-0"
            id="settings-modal-title"
          >
            Settings
          </h2>
          <Button variant="ghost" size="icon" onClick={onClose} aria-label="Close settings">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
              <path
                d="M4.5 4.5L11.5 11.5M11.5 4.5L4.5 11.5"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                fill="none"
              />
            </svg>
          </Button>
        </div>

        <div className="flex gap-0 px-5 border-b border-surface-overlay shrink-0">
          <button
            className={`min-h-11 px-3.5 bg-transparent border-b-2 font-body text-body-sm font-medium cursor-pointer transition-colors duration-150 ${
              activeTab === "general"
                ? "text-accent-action border-b-accent-action"
                : "text-text-muted border-b-transparent hover:text-text-secondary"
            }`}
            onClick={() => setActiveTab("general")}
            type="button"
          >
            General
          </button>
          <button
            className={`min-h-11 px-3.5 bg-transparent border-b-2 font-body text-body-sm font-medium cursor-pointer transition-colors duration-150 ${
              activeTab === "models"
                ? "text-accent-action border-b-accent-action"
                : "text-text-muted border-b-transparent hover:text-text-secondary"
            }`}
            onClick={() => setActiveTab("models")}
            type="button"
          >
            Models
          </button>
          <button
            className={`min-h-11 px-3.5 bg-transparent border-b-2 font-body text-body-sm font-medium cursor-pointer transition-colors duration-150 ${
              activeTab === "mcp"
                ? "text-accent-action border-b-accent-action"
                : "text-text-muted border-b-transparent hover:text-text-secondary"
            }`}
            onClick={() => setActiveTab("mcp")}
            type="button"
          >
            MCP
          </button>
          <button
            className={`min-h-11 px-3.5 bg-transparent border-b-2 font-body text-body-sm font-medium cursor-pointer transition-colors duration-150 ${
              activeTab === "data"
                ? "text-accent-action border-b-accent-action"
                : "text-text-muted border-b-transparent hover:text-text-secondary"
            }`}
            onClick={() => setActiveTab("data")}
            type="button"
          >
            Data
          </button>
        </div>

        <div className="p-5 overflow-y-auto min-h-0 bg-surface-elevated">
          {activeTab === "models" ? (
            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between gap-3">
                <h3 className="font-sans text-body-sm font-semibold text-text-secondary uppercase tracking-[0.04em] m-0">
                  Credentialed Providers
                </h3>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void onRefreshAvailability(true)}
                  disabled={isRefreshingModels}
                >
                  {isRefreshingModels ? "Refreshing..." : "Refresh models"}
                </Button>
              </div>
              <p className="m-0 text-body-sm text-text-muted">
                Provider rows are loaded from backend availability. Add credentials to make models
                selectable.
              </p>
              <ProviderSelector
                providers={providers}
                onProvidersChange={onProvidersChange}
                onRefreshAvailability={onRefreshAvailability}
              />
            </div>
          ) : activeTab === "general" ? (
            <div className="flex flex-col gap-4">
              <section className="grid gap-2">
                <div>
                  <h3 className="m-0 text-heading-sm font-medium text-text-primary">Theme</h3>
                  <p className="m-0 text-body-sm text-text-muted">
                    Dark stays the default for focused coding sessions.
                  </p>
                </div>
                <div
                  className="grid grid-cols-3 overflow-hidden rounded-lg border border-surface-overlay"
                  role="radiogroup"
                  aria-label="Theme preference"
                >
                  {THEME_OPTIONS.map((option, index) => {
                    const selected = option.value === themePreference;
                    return (
                      <button
                        key={option.value}
                        type="button"
                        role="radio"
                        aria-checked={selected}
                        tabIndex={selected ? 0 : -1}
                        ref={(el) => {
                          themeOptionRefs.current[index] = el;
                        }}
                        className={`min-h-11 px-3 py-2 text-left transition-colors duration-150 ease-out-quart ${
                          selected
                            ? "bg-surface-overlay text-accent-action"
                            : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
                        }`}
                        onClick={() => onThemePreferenceChange(option.value)}
                        onKeyDown={(event) => {
                          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                          event.preventDefault();
                          const nextIndex =
                            event.key === "ArrowLeft"
                              ? (activeThemeIndex - 1 + THEME_OPTIONS.length) % THEME_OPTIONS.length
                              : (activeThemeIndex + 1) % THEME_OPTIONS.length;
                          const nextValue = THEME_OPTIONS[nextIndex]!.value;
                          onThemePreferenceChange(nextValue);
                          themeOptionRefs.current[nextIndex]?.focus();
                        }}
                      >
                        <span className="block text-body-sm font-medium">{option.label}</span>
                        <span className="block font-mono text-caption text-text-muted">
                          {option.detail}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </section>
            </div>
          ) : activeTab === "mcp" ? (
            <McpSettingsPanel />
          ) : (
            <div className="flex flex-col gap-5">
              <section className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <h3 className="m-0 text-heading-sm font-medium text-text-primary">Archive</h3>
                    <p className="m-0 text-body-sm text-text-muted">
                      {archiveStats
                        ? `${archiveStats.archived_count} archived, ${archiveStats.expired_count} expired, ${formatBytes(archiveStats.archived_bytes)} stored`
                        : "Archive statistics unavailable"}
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void onRunArchiveMaintenance?.()}
                    disabled={archivePolicySaving || !onRunArchiveMaintenance}
                  >
                    Run cleanup
                  </Button>
                </div>
              </section>

              {archivePolicy && (
                <section className="grid gap-3">
                  <div>
                    <h3 className="m-0 text-heading-sm font-medium text-text-primary">
                      Global policy
                    </h3>
                    <p className="m-0 text-body-sm text-text-muted">
                      Default for every workspace without an override.
                    </p>
                  </div>
                  <PolicyControls
                    policy={archivePolicy}
                    disabled={archivePolicySaving || !onArchivePolicyChange}
                    onChange={(retentionDays, autoArchiveHours) =>
                      onArchivePolicyChange?.(null, retentionDays, autoArchiveHours)
                    }
                  />
                </section>
              )}

              {workspaceArchivePolicy && activeWorkspaceName && (
                <section className="grid gap-3">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <h3 className="m-0 text-heading-sm font-medium text-text-primary">
                        Workspace policy
                      </h3>
                      <p className="m-0 text-body-sm text-text-muted">
                        {workspaceArchivePolicy.uses_workspace_override
                          ? activeWorkspaceName
                          : `${activeWorkspaceName} uses the global policy`}
                      </p>
                    </div>
                    {workspaceArchivePolicy.uses_workspace_override && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => void onClearWorkspaceArchivePolicy?.()}
                        disabled={archivePolicySaving || !onClearWorkspaceArchivePolicy}
                      >
                        Use global
                      </Button>
                    )}
                  </div>
                  <PolicyControls
                    policy={workspaceArchivePolicy}
                    disabled={archivePolicySaving || !onArchivePolicyChange}
                    onChange={(retentionDays, autoArchiveHours) =>
                      onArchivePolicyChange?.(
                        workspaceArchivePolicy.workspace_id,
                        retentionDays,
                        autoArchiveHours
                      )
                    }
                  />
                </section>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function PolicyControls({
  policy,
  disabled,
  onChange,
}: {
  policy: ArchivePolicy;
  disabled: boolean;
  onChange: (retentionDays: number, autoArchiveHours: number) => void;
}) {
  return (
    <div className="grid gap-3">
      <div className="grid gap-1.5">
        <div className="text-caption font-medium uppercase tracking-[0.04em] text-text-muted">
          Retention
        </div>
        <div className="grid grid-cols-4 overflow-hidden rounded-md border border-surface-overlay">
          {RETENTION_OPTIONS.map((option) => {
            const selected = option.value === policy.retention_days;
            return (
              <button
                key={option.value}
                type="button"
                className={`min-h-9 px-2 text-body-sm transition-colors duration-150 ease-out-quart ${
                  selected
                    ? "bg-surface-overlay text-accent-action"
                    : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
                }`}
                disabled={disabled}
                onClick={() => onChange(option.value, policy.auto_archive_hours)}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </div>
      <div className="grid gap-1.5">
        <div className="text-caption font-medium uppercase tracking-[0.04em] text-text-muted">
          Auto archive
        </div>
        <div className="grid grid-cols-5 overflow-hidden rounded-md border border-surface-overlay">
          {AUTO_ARCHIVE_OPTIONS.map((option) => {
            const selected = option.value === policy.auto_archive_hours;
            return (
              <button
                key={option.value}
                type="button"
                className={`min-h-9 px-2 text-body-sm transition-colors duration-150 ease-out-quart ${
                  selected
                    ? "bg-surface-overlay text-accent-action"
                    : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
                }`}
                disabled={disabled}
                onClick={() => onChange(policy.retention_days, option.value)}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
