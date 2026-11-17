import {
  Activity,
  AlertTriangle,
  Bot,
  Check,
  CheckCircle2,
  ChevronRight,
  Copy,
  Cpu,
  Database,
  Info,
  Layers,
  PanelLeft,
  PanelLeftClose,
  PanelRight,
  PanelRightClose,
  Plus,
  RefreshCw,
  Search,
  Server,
  Shield,
  Sliders,
  Sparkles,
  Terminal,
  User,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useAutonomyPolicy } from "../hooks/useAutonomyPolicy";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { useSkills } from "../hooks/useSkills";
import { policyGlobToRegExp } from "../lib/policyGlob";
import type { ArchivePolicy, ArchiveStats, ThemePreference } from "../types";
import { CustomProvidersPanel } from "./CustomProvidersPanel";
import { McpSettingsPanel } from "./McpSettingsPanel";
import { type ProviderConfig, ProviderSelector } from "./ProviderSelector";

interface SettingsStudioProps {
  onClose: () => void;
  initialCategory: SettingsCategory;
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
  isRefreshingModels: boolean;
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

type PersonaFilter = "all" | "developer" | "agent";
export type SettingsCategory =
  | "appearance"
  | "workspace"
  | "providers"
  | "storage"
  | "autonomy"
  | "mcp"
  | "skills"
  | "guards";

interface CategoryMeta {
  id: SettingsCategory;
  name: string;
  persona: "developer" | "agent";
  icon: typeof Shield;
  badge?: string;
  badgeColor?: string;
  description: string;
}

const PRESET_COMMANDS = [
  { label: "rm -rf dist", cmd: "rm -rf dist" },
  { label: "git status", cmd: "git status" },
  { label: "git push --force", cmd: "git push origin main --force" },
  { label: "rm -rf /", cmd: "rm -rf /" },
  { label: "cargo check", cmd: "cargo check" },
];

const PRESET_PROMPTS = [
  { label: "Diagnose bug", prompt: "diagnose a memory leak in background worker" },
  { label: "Prototype UI", prompt: "prototype a new command palette layout" },
  { label: "Write unit tests", prompt: "tdd write unit tests for session manager" },
  { label: "Simplify code", prompt: "ponytail simplify this bloated helper" },
];

const QUICK_DENY_PRESETS = ["rm -rf *", "git push *--force*", "drop table*", "curl *", "dd if=*"];

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
        <div className="text-[10px] font-medium uppercase tracking-[0.04em] text-text-muted">
          Retention
        </div>
        <div className="flex gap-1 p-0.5 rounded-lg bg-surface-sunken/80">
          {RETENTION_OPTIONS.map((option) => {
            const selected = option.value === policy.retention_days;
            return (
              <button
                key={option.value}
                type="button"
                className={`flex-1 min-h-8 px-2 rounded-md text-xs font-mono transition duration-150 active:scale-[0.97] ${
                  selected
                    ? "bg-surface-elevated text-accent-structure shadow-sm font-semibold"
                    : "text-text-muted hover:text-text-secondary"
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
        <div className="text-[10px] font-medium uppercase tracking-[0.04em] text-text-muted">
          Auto archive
        </div>
        <div className="flex gap-1 p-0.5 rounded-lg bg-surface-sunken/80">
          {AUTO_ARCHIVE_OPTIONS.map((option) => {
            const selected = option.value === policy.auto_archive_hours;
            return (
              <button
                key={option.value}
                type="button"
                className={`flex-1 min-h-8 px-2 rounded-md text-xs font-mono transition duration-150 active:scale-[0.97] ${
                  selected
                    ? "bg-surface-elevated text-accent-structure shadow-sm font-semibold"
                    : "text-text-muted hover:text-text-secondary"
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

export function SettingsStudio({
  onClose,
  initialCategory,
  providers,
  onProvidersChange,
  onRefreshAvailability,
  isRefreshingModels,
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
}: SettingsStudioProps) {
  const [personaFilter, setPersonaFilter] = useState<PersonaFilter>("all");
  const [selectedCategory, setSelectedCategory] = useState<SettingsCategory>(initialCategory);
  const [searchQuery, setSearchQuery] = useState("");
  const [sidecarTab, setSidecarTab] = useState<"tester" | "preamble">("tester");
  const [copiedPreamble, setCopiedPreamble] = useState(false);

  // Emil Kowalski craft: Collapsible navigation & sidecar state
  const [isNavCollapsed, setIsNavCollapsed] = useState(false);
  const [isSidecarOpen, setIsSidecarOpen] = useState(true);

  const autonomy = useAutonomyPolicy();
  const { skills } = useSkills();
  const [newDenyRule, setNewDenyRule] = useState("");

  // Live tester state (Agent Inspector sidecar)
  const [testCommand, setTestCommand] = useState("rm -rf dist");
  const [testPrompt, setTestPrompt] = useState("diagnose a performance regression in test suite");

  const dialogRef = useRef<HTMLDivElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    openerRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      openerRef.current?.focus();
    };
  }, []);

  useFocusTrap({ active: true, containerRef: dialogRef, onEscape: onClose });

  useEffect(() => {
    setSelectedCategory(initialCategory);
  }, [initialCategory]);

  const activeProvidersCount = providers.filter((p) => p.credentialed).length;

  // Custom Providers are managed by CustomProvidersPanel; the built-in
  // ProviderSelector list must not duplicate them. Re-append them on change
  // so a built-in edit never drops the custom rows.
  const builtinProviders = useMemo(
    () => providers.filter((provider) => !provider.isCustom),
    [providers]
  );
  const customProviders = useMemo(
    () => providers.filter((provider) => provider.isCustom),
    [providers]
  );
  const customProvidersRef = useRef(customProviders);
  useEffect(() => {
    customProvidersRef.current = customProviders;
  }, [customProviders]);

  const handleBuiltinProvidersChange = useCallback(
    (updated: ProviderConfig[]) => {
      onProvidersChange([...updated, ...customProvidersRef.current]);
    },
    [onProvidersChange]
  );

  const categories: CategoryMeta[] = useMemo(
    () => [
      // Developer Persona Categories
      {
        id: "appearance",
        name: "Interface & Theme",
        persona: "developer",
        icon: Sliders,
        badge: themePreference.toUpperCase(),
        badgeColor: "text-text-muted bg-surface-overlay/80",
        description: "Visual palette, typography and dark mode",
      },
      {
        id: "workspace",
        name: "Workspace Binding",
        persona: "developer",
        icon: Activity,
        description: "Folder roots and project session affinity",
      },
      {
        id: "providers",
        name: "Providers & Credentials",
        persona: "developer",
        icon: Cpu,
        badge: `${activeProvidersCount} Active`,
        badgeColor:
          activeProvidersCount > 0
            ? "text-status-success bg-status-success/15"
            : "text-status-warning bg-status-warning/15",
        description: "Inference credentials, OAuth, and API keys",
      },
      {
        id: "storage",
        name: "Storage & Retention",
        persona: "developer",
        icon: Database,
        badge: archiveStats ? `${archiveStats.archived_count} Items` : undefined,
        badgeColor: "text-text-muted bg-surface-overlay/80",
        description: "SQLite transcripts and maintenance cleanup",
      },

      // Agent Persona Categories
      {
        id: "autonomy",
        name: "Autonomy & Guardrails",
        persona: "agent",
        icon: Shield,
        badge: autonomy.mode.toUpperCase(),
        badgeColor:
          autonomy.mode === "auto"
            ? "text-accent-signal bg-accent-signal/15"
            : "text-accent-structure bg-accent-structure/15",
        description: "Shell command approval and reject patterns",
      },
      {
        id: "skills",
        name: "Discovered Skills",
        persona: "agent",
        icon: Sparkles,
        badge: `${skills.length} Skills`,
        badgeColor: "text-accent-action bg-accent-action/15",
        description: "Prompt token matchers and preambles",
      },
      {
        id: "mcp",
        name: "MCP & Tool Registry",
        persona: "agent",
        icon: Server,
        description: "Live workspace tools and MCP servers",
      },
      {
        id: "guards",
        name: "Turn Circuit Breakers",
        persona: "agent",
        icon: AlertTriangle,
        badge: "3 / 5 Turns",
        badgeColor: "text-status-warning bg-status-warning/15",
        description: "Loop detection and verification sub-agent",
      },
    ],
    [activeProvidersCount, autonomy.mode, themePreference, archiveStats, skills.length]
  );

  const selectedCategoryMeta = useMemo(() => {
    return categories.find((c) => c.id === selectedCategory) || categories[0];
  }, [categories, selectedCategory]);

  // Filtered categories based on persona filter & search
  const visibleCategories = useMemo(() => {
    return categories.filter((cat) => {
      if (personaFilter !== "all" && cat.persona !== personaFilter) return false;
      if (!searchQuery.trim()) return true;
      const q = searchQuery.toLowerCase();
      return cat.name.toLowerCase().includes(q) || cat.description.toLowerCase().includes(q);
    });
  }, [categories, personaFilter, searchQuery]);

  // Keyboard shortcut listener for power users
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    const activeEl = document.activeElement;
    if (
      activeEl instanceof HTMLInputElement ||
      activeEl instanceof HTMLTextAreaElement ||
      (activeEl instanceof HTMLElement && activeEl.isContentEditable)
    ) {
      return;
    }

    if (e.key === "[") {
      setIsNavCollapsed((prev) => !prev);
    } else if (e.key === "]" || (e.key === "j" && (e.metaKey || e.ctrlKey))) {
      e.preventDefault();
      setIsSidecarOpen((prev) => !prev);
    }
  }, []);

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // Policy check against the user's real allow/deny rules. The built-in
  // restricted-command list lives in the backend CommandPolicy and isn't
  // re-evaluated here.
  const evaluatedCommand = useMemo(() => {
    const cmd = testCommand.trim();
    if (!cmd) return null;
    const normalizedCmd = cmd.replace(/\s+/g, " ");

    for (const pattern of autonomy.policy.reject_patterns) {
      const regex = policyGlobToRegExp(pattern);
      if (regex.test(cmd) || regex.test(normalizedCmd)) {
        return {
          badge: "DENY PATTERN",
          reason: `Matches User Deny Rule: "${pattern}"`,
          action: autonomy.mode === "auto" ? "Prompts human approval" : "Hard Denied in Ask mode",
          icon: XCircle,
          color: "text-status-error bg-status-error/10 border-status-error/20",
        };
      }
    }

    for (const pattern of autonomy.policy.allowed_patterns) {
      const regex = policyGlobToRegExp(pattern);
      if (regex.test(cmd)) {
        return {
          badge: "PRE-APPROVED",
          reason: `Matches Workspace Pre-approved Pattern: "${pattern}"`,
          action: "Runs immediately without prompting",
          icon: CheckCircle2,
          color: "text-status-success bg-status-success/10 border-status-success/20",
        };
      }
    }

    return {
      badge: "NO USER RULE",
      reason: "No user rule matched; runtime decision handled by backend CommandPolicy",
      action: "No user rule matched",
      icon: Info,
      color: "text-text-muted bg-surface-overlay/80 border-border-subtle",
    };
  }, [testCommand, autonomy.policy, autonomy.mode]);

  // Name-match preview over the real discovered skills (the backend matcher
  // owns actual prompt matching; this is a lookup hint only).
  const matchedSkills = useMemo(() => {
    const p = testPrompt.toLowerCase();
    if (!p) return [];
    return skills.filter((s) => {
      const name = s.name.toLowerCase();
      return p.includes(name) || name.split(/[-_]/).some((t) => t.length > 2 && p.includes(t));
    });
  }, [testPrompt, skills]);

  const compiledPreambleText = useMemo(() => {
    return `# Gospel Agent System Preamble
Workspace Context: ${activeWorkspaceName ?? "none"}
Autonomy Mode: ${autonomy.mode.toUpperCase()}
Deny Rules (${autonomy.policy.reject_patterns.length}): ${autonomy.policy.reject_patterns.join(", ")}

Active Skills (Matched & Injected):
${
  matchedSkills.length > 0
    ? matchedSkills.map((m) => `- /${m.name} [${m.source}]`).join("\n")
    : "- None (General purpose assistant mode)"
}`;
  }, [activeWorkspaceName, autonomy.mode, autonomy.policy.reject_patterns, matchedSkills]);

  const handleCopyPreamble = () => {
    void navigator.clipboard
      .writeText(compiledPreambleText)
      .then(() => {
        setCopiedPreamble(true);
        setTimeout(() => setCopiedPreamble(false), 1800);
      })
      .catch(() => {});
  };

  return (
    <div className="fixed inset-0 bg-scrim z-[var(--z-dialog)] flex items-center justify-center p-3 sm:p-5 animate-fade-in font-sans">
      <button
        type="button"
        className="absolute inset-0 cursor-default"
        onClick={onClose}
        aria-label="Close settings"
        tabIndex={-1}
      />
      <div
        ref={dialogRef}
        className="relative w-full max-w-[1180px] h-[90vh] max-h-[860px] flex flex-col bg-surface-base border border-white/[0.07] rounded-2xl shadow-[0_32px_80px_rgba(0,0,0,0.7)] overflow-hidden animate-slide-up"
        role="dialog"
        aria-modal="true"
        aria-labelledby="studio-harness-title"
      >
        {/* Emil Kowalski: Refined Top Header with Tonal Separation & Sidebar Toggles */}
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-white/[0.04] bg-surface-sunken/80 backdrop-blur-md shrink-0 select-none">
          <div className="flex items-center gap-3">
            {/* Left Nav Collapse Toggle */}
            <button
              type="button"
              onClick={() => setIsNavCollapsed((prev) => !prev)}
              className="p-1.5 rounded-lg text-text-muted hover:text-text-primary hover:bg-surface-elevated/70 transition duration-150 active:scale-[0.95]"
              title={isNavCollapsed ? "Expand sidebar ([)" : "Collapse sidebar ([)"}
            >
              {isNavCollapsed ? <PanelLeft size={16} /> : <PanelLeftClose size={16} />}
            </button>

            <div className="flex items-center gap-2">
              <div className="p-1 rounded-md bg-accent-action/15 text-accent-structure">
                <Layers size={14} />
              </div>
              <div className="flex items-center gap-1.5 text-xs">
                <span className="font-semibold text-text-primary">Gospel</span>
                <ChevronRight size={12} className="text-text-muted/60" />
                <span className="text-text-muted capitalize">
                  {personaFilter === "all" ? "Settings" : `${personaFilter} Lens`}
                </span>
                <ChevronRight size={12} className="text-text-muted/60" />
                <span className="font-medium text-accent-structure">
                  {selectedCategoryMeta.name}
                </span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {/* Emil Kowalski: Seamless Segmented Lens Switcher (No harsh borders) */}
            <div className="hidden md:flex items-center p-0.5 rounded-lg bg-surface-sunken/90 text-xs">
              <button
                type="button"
                onClick={() => setPersonaFilter("all")}
                className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition duration-150 active:scale-[0.97] ${
                  personaFilter === "all"
                    ? "bg-surface-elevated text-text-primary shadow-sm"
                    : "text-text-muted hover:text-text-secondary"
                }`}
                title="All categories"
              >
                All ({categories.length})
              </button>
              <button
                type="button"
                onClick={() => setPersonaFilter("developer")}
                className={`flex items-center gap-1 px-2.5 py-1 rounded-md text-[11px] font-medium transition duration-150 active:scale-[0.97] ${
                  personaFilter === "developer"
                    ? "bg-surface-elevated text-accent-structure shadow-sm font-semibold"
                    : "text-text-muted hover:text-text-secondary"
                }`}
                title="Developer Environment (1)"
              >
                <User size={11} /> Developer
              </button>
              <button
                type="button"
                onClick={() => setPersonaFilter("agent")}
                className={`flex items-center gap-1 px-2.5 py-1 rounded-md text-[11px] font-medium transition duration-150 active:scale-[0.97] ${
                  personaFilter === "agent"
                    ? "bg-surface-elevated text-accent-action shadow-sm font-semibold"
                    : "text-text-muted hover:text-text-secondary"
                }`}
                title="Agent Harness (2)"
              >
                <Bot size={11} /> Agent Harness
              </button>
            </div>

            {/* Sync Button with active press scale */}
            <button
              type="button"
              className="flex items-center gap-1.5 text-xs h-7 px-2.5 rounded-lg bg-surface-elevated/60 hover:bg-surface-elevated text-text-secondary hover:text-text-primary transition duration-150 active:scale-[0.97]"
              onClick={() => void onRefreshAvailability(true)}
              disabled={isRefreshingModels}
            >
              <RefreshCw size={12} className={isRefreshingModels ? "animate-spin" : ""} />
              <span>{isRefreshingModels ? "Syncing..." : "Sync"}</span>
            </button>

            {/* Live sync badge shown while the Inspector is collapsed */}
            {!isSidecarOpen && (
              <span className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-surface-elevated/70 text-[11px] font-mono text-status-success">
                <span className="w-1.5 h-1.5 rounded-full bg-status-success inline-block" />
                Synced · {autonomy.mode.toUpperCase()}
              </span>
            )}

            {/* Right Inspector Sidecar Toggle */}
            <button
              type="button"
              onClick={() => setIsSidecarOpen((prev) => !prev)}
              className={`flex items-center gap-1.5 h-7 px-2.5 rounded-lg text-xs font-medium transition duration-150 active:scale-[0.97] ${
                isSidecarOpen
                  ? "bg-accent-action/15 text-accent-structure"
                  : "bg-surface-elevated/60 text-text-muted hover:text-text-secondary"
              }`}
              title={isSidecarOpen ? "Hide Inspector (] or ⌘J)" : "Show Inspector (] or ⌘J)"}
            >
              <Sparkles
                size={12}
                className={isSidecarOpen ? "text-accent-structure" : "text-text-muted"}
              />
              <span className="hidden sm:inline">Inspector</span>
              {isSidecarOpen ? <PanelRightClose size={13} /> : <PanelRight size={13} />}
            </button>

            {/* Esc Close Button */}
            <button
              type="button"
              onClick={onClose}
              className="text-xs h-7 px-2.5 rounded-lg text-text-muted hover:text-text-primary hover:bg-surface-elevated/60 transition duration-150 active:scale-[0.95]"
              title="Close Settings (Esc)"
            >
              Esc
            </button>
          </div>
        </div>

        {/* 3-Column Adaptive Studio Workbench Body */}
        <div className="flex flex-1 min-h-0 overflow-hidden">
          {/* Left Navigation Rail (Fluid collapsible 240px <-> 58px) */}
          <div
            className={`flex flex-col bg-surface-sunken/40 border-r border-white/[0.04] shrink-0 transition-[width] duration-200 ease-out ${
              isNavCollapsed ? "w-[58px]" : "w-[240px]"
            }`}
          >
            {/* Search Input (Hidden in collapsed mode) */}
            {!isNavCollapsed && (
              <div className="p-3 pb-2">
                <div className="relative flex items-center">
                  <Search size={13} className="absolute left-2.5 text-text-muted" />
                  <input
                    type="text"
                    aria-label="Filter settings"
                    placeholder="Filter settings..."
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    className="w-full pl-8 pr-7 py-1.5 text-xs bg-surface-sunken/80 rounded-lg text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-1 focus:ring-accent-action transition"
                  />
                </div>
              </div>
            )}

            {/* Category Navigation Items (Emil Kowalski: Borderless Tonal Selection) */}
            <div className="flex-1 overflow-y-auto p-2 space-y-1">
              {visibleCategories.map((cat) => {
                const Icon = cat.icon;
                const isSelected = selectedCategory === cat.id;

                if (isNavCollapsed) {
                  return (
                    <button
                      key={cat.id}
                      type="button"
                      onClick={() => setSelectedCategory(cat.id)}
                      title={`${cat.name} — ${cat.description}`}
                      className={`w-full flex items-center justify-center p-2.5 rounded-xl transition duration-150 active:scale-[0.95] ${
                        isSelected
                          ? "bg-surface-elevated text-text-primary shadow-sm"
                          : "text-text-muted hover:text-text-primary hover:bg-surface-elevated/40"
                      }`}
                    >
                      <Icon
                        size={17}
                        className={
                          isSelected
                            ? cat.persona === "agent"
                              ? "text-accent-action"
                              : "text-accent-structure"
                            : "text-text-muted"
                        }
                      />
                    </button>
                  );
                }

                return (
                  <button
                    key={cat.id}
                    type="button"
                    onClick={() => setSelectedCategory(cat.id)}
                    className={`w-full flex items-center justify-between p-2 rounded-xl text-left transition duration-150 active:scale-[0.98] ${
                      isSelected
                        ? "bg-surface-elevated text-text-primary shadow-sm"
                        : "text-text-secondary hover:bg-surface-elevated/40 hover:text-text-primary"
                    }`}
                  >
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Icon
                        size={15}
                        className={
                          isSelected
                            ? cat.persona === "agent"
                              ? "text-accent-action"
                              : "text-accent-structure"
                            : "text-text-muted"
                        }
                      />
                      <div className="truncate">
                        <div className="text-xs font-medium truncate">{cat.name}</div>
                        <div className="text-[10px] text-text-muted truncate">
                          {cat.description}
                        </div>
                      </div>
                    </div>
                    {cat.badge && (
                      <span
                        className={`text-[10px] font-mono px-1.5 py-0.5 rounded-md shrink-0 ml-1 ${cat.badgeColor}`}
                      >
                        {cat.badge}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>

            {/* Bottom Status Footnote */}
            <div className="p-3 border-t border-white/[0.04] text-[11px] text-text-muted flex items-center justify-between">
              {!isNavCollapsed && <span className="font-mono text-[10px]">v0.1.0-alpha.1</span>}
              <span
                className={`inline-flex items-center gap-1.5 text-status-success font-mono text-[10px] ${isNavCollapsed ? "mx-auto" : ""}`}
              >
                <span className="w-1.5 h-1.5 rounded-full bg-status-success inline-block animate-pulse" />
                {!isNavCollapsed && "Harness Live"}
              </span>
            </div>
          </div>

          {/* Middle Main Canvas (Border-free Tonal Hierarchy & Spacious Flow) */}
          <div className="flex-1 overflow-y-auto p-6 lg:p-8 bg-surface-base">
            {/* CATEGORY: AUTONOMY & GUARDRAILS */}
            {selectedCategory === "autonomy" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="text-base font-semibold text-text-primary m-0">
                      Agent Autonomy Policy (ADR-0014)
                    </h3>
                    <p className="text-xs text-text-muted mt-0.5">
                      Controls permissions for shell, git, and custom tool executions.
                    </p>
                  </div>
                  <span className="text-[11px] font-mono px-2 py-0.5 rounded-md bg-accent-action/10 text-accent-structure">
                    Global Policy
                  </span>
                </div>

                {/* Mode Selector (Emil Kowalski: Tonal Depth instead of Border Boxes) */}
                <div className="grid grid-cols-2 gap-3">
                  <button
                    type="button"
                    onClick={() => void autonomy.setMode("ask")}
                    className={`p-4 rounded-xl text-left transition duration-150 active:scale-[0.98] ${
                      autonomy.mode === "ask"
                        ? "bg-surface-elevated text-text-primary shadow-md ring-1 ring-accent-action/50"
                        : "bg-surface-sunken/40 text-text-muted hover:bg-surface-sunken/70 hover:text-text-secondary"
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1.5">
                      <div className="text-xs font-semibold text-text-primary flex items-center gap-1.5">
                        <Shield size={14} className="text-accent-structure" />
                        Ask Mode (Default)
                      </div>
                      {autonomy.mode === "ask" && (
                        <span className="px-1.5 py-0.5 rounded bg-accent-action/20 text-accent-structure text-[10px] font-mono font-bold">
                          ACTIVE
                        </span>
                      )}
                    </div>
                    <p className="text-[11px] text-text-muted m-0 leading-relaxed">
                      Requires one-time approval for mutating commands. Blocks dangerous restricted
                      commands outright.
                    </p>
                  </button>

                  <button
                    type="button"
                    onClick={() => void autonomy.setMode("auto")}
                    className={`p-4 rounded-xl text-left transition duration-150 active:scale-[0.98] ${
                      autonomy.mode === "auto"
                        ? "bg-surface-elevated text-text-primary shadow-md ring-1 ring-accent-signal/50"
                        : "bg-surface-sunken/40 text-text-muted hover:bg-surface-sunken/70 hover:text-text-secondary"
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1.5">
                      <div className="text-xs font-semibold text-accent-signal flex items-center gap-1.5">
                        <AlertTriangle size={14} className="text-accent-signal" />
                        Auto Mode
                      </div>
                      {autonomy.mode === "auto" && (
                        <span className="px-1.5 py-0.5 rounded bg-accent-signal/20 text-accent-signal text-[10px] font-mono font-bold">
                          ACTIVE
                        </span>
                      )}
                    </div>
                    <p className="text-[11px] text-text-muted m-0 leading-relaxed">
                      Executes commands autonomously except those matching explicit User Deny
                      Rules. External-path requests always require per-request approval, and MCP
                      tools require explicit server/tool allow rules.
                    </p>
                  </button>
                </div>

                {/* Deny & Reject Rules Manager (Tonal Inset Surfaces) */}
                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">User Deny Rules</div>
                      <div className="text-[11px] text-text-muted">
                        Commands matching these patterns always prompt, even in Auto mode.
                      </div>
                    </div>
                    <span className="text-[10px] font-mono text-text-muted">
                      {autonomy.policy.reject_patterns.length} Rules Active
                    </span>
                  </div>

                  {/* 1-Click Quick Preset Chips */}
                  <div className="flex flex-wrap items-center gap-1.5 pt-1">
                    <span className="text-[10px] font-mono text-text-muted">Quick Presets:</span>
                    {QUICK_DENY_PRESETS.map((preset) => {
                      const alreadyAdded = autonomy.policy.reject_patterns.includes(preset);
                      return (
                        <button
                          key={preset}
                          type="button"
                          onClick={() => {
                            if (!alreadyAdded) void autonomy.addRejectRule(preset);
                          }}
                          disabled={alreadyAdded}
                          className={`px-2 py-0.5 rounded-md text-[10px] font-mono transition duration-150 active:scale-[0.95] ${
                            alreadyAdded
                              ? "bg-surface-sunken/30 text-text-muted/40 cursor-not-allowed"
                              : "bg-surface-sunken/80 text-text-secondary hover:bg-surface-overlay hover:text-text-primary cursor-pointer"
                          }`}
                        >
                          + {preset}
                        </button>
                      );
                    })}
                  </div>

                  {/* Rules list with soft inset containers */}
                  <div className="space-y-1.5 pt-1">
                    {autonomy.policy.reject_patterns.map((rule) => (
                      <div
                        key={rule}
                        className="flex items-center justify-between px-3 py-1.5 rounded-lg bg-surface-sunken/60 font-mono text-xs text-text-secondary group"
                      >
                        <span className="text-status-error">{rule}</span>
                        <button
                          type="button"
                          onClick={() => void autonomy.removeRule(rule)}
                          className="h-6 px-1.5 text-[10px] text-text-muted hover:text-status-error opacity-60 group-hover:opacity-100 transition active:scale-[0.95]"
                        >
                          Remove
                        </button>
                      </div>
                    ))}
                  </div>

                  <div className="flex gap-2 pt-1">
                    <input
                      type="text"
                      aria-label="Add custom deny pattern"
                      placeholder="Add custom pattern (e.g. drop table*, curl *)..."
                      value={newDenyRule}
                      onChange={(e) => setNewDenyRule(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && newDenyRule.trim()) {
                          void autonomy.addRejectRule(newDenyRule.trim());
                          setNewDenyRule("");
                        }
                      }}
                      className="flex-1 px-3 py-1.5 text-xs bg-surface-sunken/80 rounded-lg text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-1 focus:ring-accent-action font-mono transition"
                    />
                    <button
                      type="button"
                      onClick={() => {
                        if (newDenyRule.trim()) {
                          void autonomy.addRejectRule(newDenyRule.trim());
                          setNewDenyRule("");
                        }
                      }}
                      className="flex items-center gap-1 text-xs h-8 px-3 rounded-lg bg-surface-overlay/80 hover:bg-surface-overlay text-text-secondary hover:text-text-primary transition active:scale-[0.97]"
                    >
                      <Plus size={12} />
                      <span>Add Rule</span>
                    </button>
                  </div>
                </div>

                {/* Pre-approved Tools Tonal Inset */}
                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-2">
                  <div className="flex items-center justify-between text-xs font-semibold text-text-primary">
                    <span className="flex items-center gap-1.5 text-status-success">
                      <CheckCircle2 size={13} /> Pre-approved Workspace Commands
                    </span>
                    <span className="text-[10px] font-mono text-text-muted">
                      {autonomy.policy.allowed_patterns.length} Auto-allowed
                    </span>
                  </div>
                  {autonomy.policy.allowed_patterns.length > 0 ? (
                    <div className="grid grid-cols-2 gap-2 text-xs font-mono text-text-secondary">
                      {autonomy.policy.allowed_patterns.map((pattern) => (
                        <div
                          key={pattern}
                          className="p-2 rounded-lg bg-surface-sunken/60 flex items-center justify-between gap-2 group"
                        >
                          <span className="truncate">{pattern}</span>
                          <button
                            type="button"
                            onClick={() => void autonomy.removeRule(pattern)}
                            className="text-[10px] text-text-muted hover:text-status-error opacity-60 group-hover:opacity-100 transition active:scale-[0.95] shrink-0"
                          >
                            Remove
                          </button>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-[11px] text-text-muted m-0">
                      No workspace allow rules yet. Commands approved with "Always allow" appear
                      here.
                    </p>
                  )}
                </div>
              </div>
            )}

            {/* CATEGORY: PROVIDERS & CREDENTIALS (Live ProviderSelector Integration) */}
            {selectedCategory === "providers" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="text-base font-semibold text-text-primary m-0">
                      Inference Providers & API Keys
                    </h3>
                    <p className="text-xs text-text-muted mt-0.5">
                      OAuth sign-in, API keys, availability checks, and active model catalogs.
                    </p>
                  </div>
                  <span className="text-[10px] font-mono text-text-muted">
                    ~/.config/gospel/credentials.json
                  </span>
                </div>

                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-3">
                  <CustomProvidersPanel
                    providers={providers}
                    onRefreshAvailability={onRefreshAvailability}
                    isRefreshingModels={isRefreshingModels}
                  />
                  <div className="text-xs font-semibold text-text-primary">Built-in providers</div>
                  <ProviderSelector
                    providers={builtinProviders}
                    onProvidersChange={handleBuiltinProvidersChange}
                    onRefreshAvailability={onRefreshAvailability}
                  />
                </div>
              </div>
            )}

            {/* CATEGORY: INTERFACE & THEME */}
            {selectedCategory === "appearance" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">
                    Interface & Visual Theme
                  </h3>
                  <p className="text-xs text-text-muted mt-0.5">
                    Customize display preferences and color palette.
                  </p>
                </div>

                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-3">
                  <div className="text-xs font-semibold text-text-primary">Theme Palette</div>
                  <div className="grid grid-cols-3 gap-3">
                    {(["dark", "light", "system"] as ThemePreference[]).map((mode) => (
                      <button
                        key={mode}
                        type="button"
                        onClick={() => onThemePreferenceChange(mode)}
                        className={`p-3.5 rounded-xl text-left transition duration-150 active:scale-[0.98] ${
                          themePreference === mode
                            ? "bg-surface-overlay text-text-primary ring-1 ring-accent-action/50 shadow-sm"
                            : "bg-surface-sunken/60 text-text-muted hover:text-text-secondary hover:bg-surface-sunken/90"
                        }`}
                      >
                        <div className="text-xs font-semibold capitalize">{mode}</div>
                        <div className="text-[10px] text-text-muted mt-1">
                          {mode === "dark"
                            ? "Geist Dark (Default)"
                            : mode === "light"
                              ? "Crisp high-contrast light"
                              : "Synchronize with macOS"}
                        </div>
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            )}

            {/* CATEGORY: WORKSPACE BINDING */}
            {selectedCategory === "workspace" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">
                    Workspace & Git Context
                  </h3>
                  <p className="text-xs text-text-muted mt-0.5">
                    Active codebase affinity and execution directory.
                  </p>
                </div>

                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="text-xs font-semibold text-text-primary">Active Workspace</div>
                  </div>
                  <div className="p-2.5 rounded-lg bg-surface-sunken/70 font-mono text-xs text-text-secondary">
                    {activeWorkspaceName ?? "No active workspace"}
                  </div>
                </div>
              </div>
            )}

            {/* CATEGORY: STORAGE & RETENTION (Full PolicyControls Integration) */}
            {selectedCategory === "storage" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">
                    Storage, Retention & Traces
                  </h3>
                  <p className="text-xs text-text-muted mt-0.5">
                    SQLite database storage and automated session pruning.
                  </p>
                </div>

                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">
                        Session Archive & Traces
                      </div>
                      <div className="text-[11px] text-text-muted mt-0.5">
                        {archiveStats
                          ? `${archiveStats.archived_count} archived turns, ${archiveStats.expired_count} expired, ${formatBytes(archiveStats.archived_bytes)} stored`
                          : "Archive statistics unavailable"}
                      </div>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void onRunArchiveMaintenance?.()}
                      disabled={archivePolicySaving || !onRunArchiveMaintenance}
                      className="text-xs h-8 border-transparent bg-surface-sunken/80 hover:bg-surface-overlay active:scale-[0.97]"
                    >
                      Run Cleanup
                    </Button>
                  </div>

                  {archivePolicy && (
                    <div className="pt-3 border-t border-white/[0.04] space-y-2">
                      <div className="text-xs font-semibold text-text-primary">
                        Global Retention Policy
                      </div>
                      <div className="text-[11px] text-text-muted">
                        Default for every workspace without an override.
                      </div>
                      <PolicyControls
                        policy={archivePolicy}
                        disabled={archivePolicySaving || !onArchivePolicyChange}
                        onChange={(retentionDays, autoArchiveHours) =>
                          onArchivePolicyChange?.(null, retentionDays, autoArchiveHours)
                        }
                      />
                    </div>
                  )}

                  {workspaceArchivePolicy && activeWorkspaceName && (
                    <div className="pt-3 border-t border-white/[0.04] space-y-2">
                      <div className="flex items-center justify-between">
                        <div>
                          <div className="text-xs font-semibold text-text-primary">
                            Workspace Policy Override
                          </div>
                          <div className="text-[11px] text-text-muted">
                            {workspaceArchivePolicy.uses_workspace_override
                              ? activeWorkspaceName
                              : `${activeWorkspaceName} uses the global policy`}
                          </div>
                        </div>
                        {workspaceArchivePolicy.uses_workspace_override && (
                          <button
                            type="button"
                            onClick={() => void onClearWorkspaceArchivePolicy?.()}
                            disabled={archivePolicySaving || !onClearWorkspaceArchivePolicy}
                            className="text-xs text-accent-structure hover:text-white px-2 py-1 rounded bg-surface-overlay/80"
                          >
                            Use Global
                          </button>
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
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* CATEGORY: DISCOVERED SKILLS */}
            {selectedCategory === "skills" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">
                    Discovered Skills & Prompts
                  </h3>
                  <p className="text-xs text-text-muted mt-0.5">
                    Skills match user prompts and dynamically inject instructions into the preamble.
                  </p>
                </div>

                <div className="space-y-2">
                  {skills.length > 0 ? (
                    skills.map((s) => (
                      <div
                        key={s.name}
                        className="p-3 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] flex items-center justify-between text-xs"
                      >
                        <div>
                          <div className="flex items-center gap-2">
                            <span className="font-mono font-semibold text-accent-structure">
                              /{s.name}
                            </span>
                            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-surface-sunken/80 text-text-muted">
                              {s.source}
                            </span>
                          </div>
                          <div className="text-[11px] text-text-muted mt-0.5">{s.description}</div>
                        </div>
                        <span className="text-[10px] font-mono text-status-success">Invocable</span>
                      </div>
                    ))
                  ) : (
                    <p className="text-[11px] text-text-muted m-0">
                      No user-invocable skills discovered.
                    </p>
                  )}
                </div>
              </div>
            )}

            {/* CATEGORY: MCP & TOOLS (Live McpSettingsPanel Integration) */}
            {selectedCategory === "mcp" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">
                    Model Context Protocol (MCP)
                  </h3>
                  <p className="text-xs text-text-muted mt-0.5">
                    Installed tool servers providing filesystem, git, and custom capability
                    discovery.
                  </p>
                </div>

                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03]">
                  <McpSettingsPanel />
                </div>
              </div>
            )}

            {/* CATEGORY: TURN CIRCUIT BREAKERS */}
            {selectedCategory === "guards" && (
              <div className="max-w-[760px] mx-auto space-y-6 animate-fade-in">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">
                    Turn Circuit Breakers & Verification
                  </h3>
                  <p className="text-xs text-text-muted mt-0.5">
                    Safeguards preventing token runaways and silent regressions.
                  </p>
                </div>

                <div className="p-4 rounded-xl bg-surface-elevated/70 shadow-[0_2px_12px_rgba(0,0,0,0.25)] border border-white/[0.03] space-y-3">
                  <div className="text-xs font-semibold text-text-primary">
                    Loop Detection Rules
                  </div>
                  <div className="grid grid-cols-2 gap-3 text-xs text-text-muted">
                    <div className="p-3 rounded-lg bg-surface-sunken/60">
                      <span className="font-semibold text-text-secondary">Loop Warning:</span> 3
                      identical tool calls
                      <div className="text-[10px] text-text-muted mt-1">
                        Alerts user with caution event
                      </div>
                    </div>
                    <div className="p-3 rounded-lg bg-surface-sunken/60">
                      <span className="font-semibold text-text-secondary">Controlled Stop:</span> 5
                      calls
                      <div className="text-[10px] text-text-muted mt-1">
                        Cleanly stops without corrupting history
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Right Live Sidecar (Collapsible 310px <-> 0px) */}
          {isSidecarOpen && (
            <div className="w-[310px] flex flex-col bg-surface-sunken/40 shrink-0 p-4 border-l border-white/[0.04] space-y-3 overflow-y-auto animate-fade-in">
              {/* Sidecar Tab Switcher with dismiss button */}
              <div className="flex items-center justify-between pb-2 border-b border-white/[0.04]">
                <div className="flex items-center gap-1.5">
                  <Sparkles size={14} className="text-accent-structure" />
                  <span className="text-xs font-semibold text-text-primary">Agent Inspector</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <div className="inline-flex p-0.5 rounded-lg bg-surface-sunken/80 text-[10px]">
                    <button
                      type="button"
                      onClick={() => setSidecarTab("tester")}
                      className={`px-2 py-0.5 rounded-md transition duration-150 active:scale-[0.95] ${
                        sidecarTab === "tester"
                          ? "bg-surface-elevated text-text-primary font-medium shadow-sm"
                          : "text-text-muted hover:text-text-secondary"
                      }`}
                    >
                      Live Tester
                    </button>
                    <button
                      type="button"
                      onClick={() => setSidecarTab("preamble")}
                      className={`px-2 py-0.5 rounded-md transition duration-150 active:scale-[0.95] ${
                        sidecarTab === "preamble"
                          ? "bg-surface-elevated text-text-primary font-medium shadow-sm"
                          : "text-text-muted hover:text-text-secondary"
                      }`}
                    >
                      Preamble
                    </button>
                  </div>
                  <button
                    type="button"
                    onClick={() => setIsSidecarOpen(false)}
                    className="p-1 rounded-md text-text-muted hover:text-text-primary hover:bg-surface-elevated/60 transition active:scale-[0.95]"
                    title="Collapse Inspector (] or ⌘J)"
                  >
                    <PanelRightClose size={13} />
                  </button>
                </div>
              </div>

              {sidecarTab === "tester" ? (
                <div className="space-y-4">
                  {/* Reactive Command Checker */}
                  <div className="space-y-1.5">
                    <div className="text-[11px] font-medium text-text-secondary flex items-center justify-between">
                      <span>Quick Command Classifier</span>
                      <Terminal size={12} className="text-text-muted" />
                    </div>
                    <input
                      type="text"
                      aria-label="Quick command classifier input"
                      value={testCommand}
                      onChange={(e) => setTestCommand(e.target.value)}
                      placeholder="Type command e.g. git push --force..."
                      className="w-full px-2.5 py-1 text-[11px] bg-surface-sunken/80 rounded-md font-mono text-text-primary focus:outline-none focus:ring-1 focus:ring-accent-action transition"
                    />

                    {/* Preset Command Chips for 1-Click Testing */}
                    <div className="flex flex-wrap gap-1 pt-0.5">
                      {PRESET_COMMANDS.map((chip) => (
                        <button
                          key={chip.label}
                          type="button"
                          onClick={() => setTestCommand(chip.cmd)}
                          className="px-1.5 py-0.5 rounded bg-surface-sunken/80 text-[10px] font-mono text-text-muted hover:text-text-primary hover:bg-surface-overlay active:scale-[0.95] transition"
                        >
                          {chip.label}
                        </button>
                      ))}
                    </div>

                    {evaluatedCommand && (
                      <div
                        className={`p-2 rounded-lg border ${evaluatedCommand.color} text-[11px] space-y-1`}
                      >
                        <div className="flex items-center justify-between font-semibold">
                          <div className="flex items-center gap-1.5">
                            <evaluatedCommand.icon size={13} />
                            <span>{evaluatedCommand.action}</span>
                          </div>
                          <span className="text-[9px] font-mono uppercase px-1 rounded bg-black/20 font-bold">
                            {evaluatedCommand.badge}
                          </span>
                        </div>
                        <div className="text-[10px] opacity-80 pl-4">{evaluatedCommand.reason}</div>
                      </div>
                    )}
                  </div>

                  {/* Reactive Skill Matcher */}
                  <div className="space-y-1.5 pt-2 border-t border-white/[0.04]">
                    <div className="text-[11px] font-medium text-text-secondary flex items-center justify-between">
                      <span>Quick Skill Matcher</span>
                      <Sparkles size={12} className="text-accent-action" />
                    </div>
                    <input
                      type="text"
                      aria-label="Quick skill matcher input"
                      value={testPrompt}
                      onChange={(e) => setTestPrompt(e.target.value)}
                      placeholder="Type sample prompt..."
                      className="w-full px-2.5 py-1 text-[11px] bg-surface-sunken/80 rounded-md text-text-primary focus:outline-none focus:ring-1 focus:ring-accent-action transition"
                    />

                    {/* Preset Prompt Chips for 1-Click Testing */}
                    <div className="flex flex-wrap gap-1 pt-0.5">
                      {PRESET_PROMPTS.map((chip) => (
                        <button
                          key={chip.label}
                          type="button"
                          onClick={() => setTestPrompt(chip.prompt)}
                          className="px-1.5 py-0.5 rounded bg-surface-sunken/80 text-[10px] text-text-muted hover:text-text-primary hover:bg-surface-overlay active:scale-[0.95] transition"
                        >
                          {chip.label}
                        </button>
                      ))}
                    </div>

                    <div className="space-y-1 pt-1">
                      {matchedSkills.map((m) => (
                        <div
                          key={m.name}
                          className="flex items-center justify-between px-2 py-1 rounded-md bg-surface-sunken/60 text-[11px]"
                        >
                          <span className="font-mono text-accent-structure font-medium">
                            /{m.name}
                          </span>
                          <span className="font-mono text-[10px] text-text-muted">{m.source}</span>
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* Live Status Indicators */}
                  <div className="p-2.5 rounded-lg bg-surface-sunken/60 space-y-1 text-[11px]">
                    <div className="flex justify-between text-text-muted">
                      <span>Active Autonomy:</span>
                      <strong className="text-text-primary uppercase font-mono">
                        {autonomy.mode}
                      </strong>
                    </div>
                    <div className="flex justify-between text-text-muted">
                      <span>Denied Patterns:</span>
                      <strong className="text-text-primary font-mono">
                        {autonomy.policy.reject_patterns.length} rules
                      </strong>
                    </div>
                    <div className="flex justify-between text-text-muted">
                      <span>Turn Budget Cap:</span>
                      <strong className="text-text-primary font-mono">5 Calls (Stop)</strong>
                    </div>
                  </div>
                </div>
              ) : (
                /* Compiled Preamble View with Copy & Token Gauge */
                <div className="flex-1 flex flex-col space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] font-mono uppercase text-text-muted">
                      Preamble Output
                    </span>
                    <button
                      type="button"
                      onClick={handleCopyPreamble}
                      className="flex items-center gap-1 text-[10px] font-mono text-accent-structure hover:text-white transition active:scale-[0.95]"
                    >
                      {copiedPreamble ? (
                        <Check size={11} className="text-status-success" />
                      ) : (
                        <Copy size={11} />
                      )}
                      <span>{copiedPreamble ? "Copied!" : "Copy"}</span>
                    </button>
                  </div>

                  {/* Preamble Size */}
                  <div className="p-2 rounded-lg bg-surface-sunken/60 space-y-1">
                    <div className="flex justify-between text-[10px] font-mono text-text-muted">
                      <span>Preamble Size</span>
                      <span className="text-text-secondary font-semibold">
                        {compiledPreambleText.length} chars
                      </span>
                    </div>
                  </div>

                  <pre className="flex-1 p-2.5 rounded-lg bg-surface-sunken/60 font-mono text-[10px] text-text-secondary whitespace-pre-wrap leading-relaxed overflow-auto">
                    {compiledPreambleText}
                  </pre>
                </div>
              )}

              <div className="mt-auto pt-3 border-t border-white/[0.04] text-[10px] text-text-muted flex items-center gap-1.5">
                <Info size={12} className="shrink-0 text-accent-structure" />
                Tuning rules immediately impacts the next agent turn.
              </div>
            </div>
          )}
        </div>

        {/* Emil Kowalski: Refined Bottom Status & Shortcut Bar */}
        <div className="px-4 py-2 border-t border-white/[0.04] bg-surface-sunken/80 backdrop-blur-md flex items-center justify-between text-[11px] text-text-muted shrink-0 select-none">
          <div className="flex items-center gap-2 font-mono text-[10px]">
            <span className="text-text-muted">Context:</span>
            <span className="text-text-secondary font-semibold">
              gospel › preferences › {selectedCategory}
            </span>
          </div>

          <div className="hidden sm:flex items-center gap-3 text-[10px] font-mono">
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 rounded bg-surface-overlay/80 text-text-secondary">
                [
              </kbd>
              <span>Nav Rail</span>
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 rounded bg-surface-overlay/80 text-text-secondary">
                ]
              </kbd>
              <span>Inspector</span>
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 rounded bg-surface-overlay/80 text-text-secondary">
                Esc
              </kbd>
              <span>Close</span>
            </span>
          </div>

          <div className="flex items-center gap-1.5 font-mono text-[10px] text-status-success">
            <span className="w-1.5 h-1.5 rounded-full bg-status-success inline-block" />
            <span>Synced · {autonomy.mode.toUpperCase()}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
