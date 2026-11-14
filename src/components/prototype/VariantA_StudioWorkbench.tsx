import { useState, useMemo } from "react";
import {
  Shield,
  Cpu,
  Server,
  Database,
  Sliders,
  Search,
  Sparkles,
  Terminal,
  Activity,
  CheckCircle2,
  AlertTriangle,
  RefreshCw,
  Key,
  Info,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ProviderConfig } from "../ProviderSelector";
import type { ArchivePolicy, ArchiveStats, ThemePreference } from "../../types";
import { useAutonomyPolicy } from "../../hooks/useAutonomyPolicy";

interface VariantAProps {
  onClose: () => void;
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

type CategoryId = "overview" | "autonomy" | "providers" | "mcp" | "skills" | "data";

export function VariantA_StudioWorkbench({
  onClose,
  providers,
  onRefreshAvailability,
  isRefreshingModels,
  themePreference,
  onThemePreferenceChange,
  archiveStats,
  activeWorkspaceName = "Current Project",
  onRunArchiveMaintenance,
  archivePolicySaving = false,
}: VariantAProps) {
  const [selectedCategory, setSelectedCategory] = useState<CategoryId>("autonomy");
  const [searchQuery, setSearchQuery] = useState("");
  const autonomy = useAutonomyPolicy();
  const [newRule, setNewRule] = useState("");

  const activeProvidersCount = providers.filter((p) => p.credentialed).length;

  const categories = useMemo(() => [
    {
      id: "overview" as CategoryId,
      name: "System Overview",
      icon: Activity,
      badge: "Healthy",
      badgeColor: "text-status-success bg-status-success/10",
      description: "Runtime health, environment, and theme",
    },
    {
      id: "autonomy" as CategoryId,
      name: "Agent Autonomy",
      icon: Shield,
      badge: autonomy.mode.toUpperCase(),
      badgeColor: autonomy.mode === "auto" ? "text-accent-signal bg-accent-signal/10" : "text-accent-structure bg-accent-structure/10",
      description: "Command approval policy and shell guardrails",
    },
    {
      id: "providers" as CategoryId,
      name: "Providers & Models",
      icon: Cpu,
      badge: `${activeProvidersCount} Active`,
      badgeColor: "text-text-secondary bg-surface-overlay",
      description: "LLM endpoints, API keys, and model routes",
    },
    {
      id: "mcp" as CategoryId,
      name: "MCP & Tools",
      icon: Server,
      badge: "2 Connected",
      badgeColor: "text-text-secondary bg-surface-overlay",
      description: "Model Context Protocol servers and live tools",
    },
    {
      id: "skills" as CategoryId,
      name: "Skills & Preamble",
      icon: Sparkles,
      badge: "8 Loaded",
      badgeColor: "text-text-secondary bg-surface-overlay",
      description: "Agent directives and system prompt extensions",
    },
    {
      id: "data" as CategoryId,
      name: "Storage & Traces",
      icon: Database,
      badge: archiveStats ? `${archiveStats.archived_count} Items` : "14.2 MB",
      badgeColor: "text-text-muted bg-surface-overlay",
      description: "Session retention, SQLite quotas, and maintenance",
    },
  ], [activeProvidersCount, autonomy.mode, archiveStats]);

  const filteredCategories = useMemo(() => {
    if (!searchQuery.trim()) return categories;
    const q = searchQuery.toLowerCase();
    return categories.filter(
      (c) => c.name.toLowerCase().includes(q) || c.description.toLowerCase().includes(q)
    );
  }, [categories, searchQuery]);

  // Derived live agent context based on selected category
  const agentImpactPreview = useMemo(() => {
    switch (selectedCategory) {
      case "autonomy":
        return {
          title: "Harness Safety Matrix",
          summary: `Mode is set to "${autonomy.mode}". Mutating commands ${
            autonomy.mode === "ask" ? "require one-time developer confirmation" : "run without approval"
          }.`,
          preambleSnippet: `## Safety Policy\n- Mode: ${autonomy.mode.toUpperCase()}\n- Blocked Patterns: ${
            autonomy.policy.reject_patterns.join(", ") || "None"
          }\n- External paths: Always require manual approval`,
          tokenImpact: "~180 prompt tokens injected",
        };
      case "providers":
        return {
          title: "Inference Router Impact",
          summary: `${activeProvidersCount} credentialed providers registered. Model picker exposes available models with zero fallback latency.`,
          preambleSnippet: `## Inference Identity\n- Active Registry: Anthropic, Gemini, Copilot\n- Tool Calling: Native Function Calling enabled\n- Stream Format: SSE Chunked with delta reasoning`,
          tokenImpact: "Zero preamble overhead (resolved per-session)",
        };
      case "mcp":
        return {
          title: "Agent Tool Capabilities",
          summary: "MCP servers expose custom tools to the LLM turn loop. Tools are dynamically registered in the agent's function-calling tool set.",
          preambleSnippet: `## Available Tool Namespaces\n- live_workspace: read_file, source_edit, search\n- mcp_servers: git-context, filesystem-inspect\n- network: Scoped HTTP only (bounty authorized)`,
          tokenImpact: "~450 tokens for tool definitions",
        };
      case "skills":
        return {
          title: "System Preamble Composition",
          summary: "Skills match user prompts dynamically and inject domain knowledge or workflow constraints into the agent preamble.",
          preambleSnippet: `## Active Skills Matcher\n- Top-3 scored matches injected per turn\n- Workspace skills (.agents/skills) override global\n- Manual /slash commands force full preamble injection`,
          tokenImpact: "Dynamic: 200 - 1,200 tokens per turn",
        };
      default:
        return {
          title: "Agent Environment Status",
          summary: "Gospel orchestrates the agent turn loop across the active workspace. Harness state persists in SQLite and local config.",
          preambleSnippet: `## Workspace Context\n- Active Path: ${activeWorkspaceName}\n- Git Status: Branch main (Clean)\n- Trace Log Quota: 250 MB max`,
          tokenImpact: "Baseline harness: ~240 tokens",
        };
    }
  }, [selectedCategory, autonomy.mode, autonomy.policy.reject_patterns, activeProvidersCount, activeWorkspaceName]);

  return (
    <div className="fixed inset-0 bg-scrim z-[var(--z-dialog)] flex items-center justify-center p-4 sm:p-6 animate-fade-in">
      <div
        className="relative w-full max-w-[1040px] h-[85vh] max-h-[760px] flex flex-col bg-surface-base border border-surface-overlay rounded-xl shadow-[0_24px_50px_rgba(0,0,0,0.45)] overflow-hidden animate-slide-up"
        role="dialog"
        aria-modal="true"
        aria-labelledby="studio-settings-title"
      >
        {/* Studio Top Header */}
        <div className="flex items-center justify-between px-6 py-3.5 border-b border-surface-line bg-surface-sunken shrink-0">
          <div className="flex items-center gap-3">
            <div className="p-1.5 rounded-md bg-accent-action/10 text-accent-structure">
              <Sliders size={18} />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 id="studio-settings-title" className="text-body font-semibold text-text-primary m-0">
                  Settings & Harness Studio
                </h2>
                <span className="text-[11px] font-mono px-2 py-0.5 rounded bg-surface-overlay text-text-muted border border-surface-line">
                  {activeWorkspaceName}
                </span>
              </div>
              <p className="text-[11px] text-text-muted m-0">
                Configure developer environment, model dispatch, and agent execution policies.
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              className="text-xs h-8 border-surface-line hover:bg-surface-elevated text-text-secondary"
              onClick={() => void onRefreshAvailability(true)}
              disabled={isRefreshingModels}
            >
              <RefreshCw size={13} className={`mr-1.5 ${isRefreshingModels ? "animate-spin" : ""}`} />
              {isRefreshingModels ? "Syncing..." : "Sync Backend"}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={onClose}
              className="text-xs h-8 px-2.5 text-text-muted hover:text-text-primary"
            >
              Close (Esc)
            </Button>
          </div>
        </div>

        {/* Studio 3-Column Body */}
        <div className="flex flex-1 min-h-0 divide-x divide-surface-line">
          {/* Left Navigation Rail (260px) */}
          <div className="w-[260px] flex flex-col bg-surface-sunken/60 shrink-0">
            {/* Search */}
            <div className="p-3 border-b border-surface-line">
              <div className="relative">
                <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-muted" />
                <input
                  type="text"
                  placeholder="Filter settings..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="w-full pl-8 pr-3 py-1.5 text-xs bg-surface-base border border-surface-line rounded-md text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent-action"
                />
              </div>
            </div>

            {/* Category list */}
            <div className="flex-1 overflow-y-auto p-2 space-y-1">
              {filteredCategories.map((cat) => {
                const Icon = cat.icon;
                const isSelected = selectedCategory === cat.id;
                return (
                  <button
                    key={cat.id}
                    type="button"
                    onClick={() => setSelectedCategory(cat.id)}
                    className={`w-full flex items-center justify-between p-2.5 rounded-lg text-left transition-all ${
                      isSelected
                        ? "bg-surface-elevated text-text-primary shadow-sm border border-surface-line"
                        : "text-text-secondary hover:bg-surface-elevated/40 hover:text-text-primary border border-transparent"
                    }`}
                  >
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Icon
                        size={16}
                        className={isSelected ? "text-accent-structure" : "text-text-muted"}
                      />
                      <div className="truncate">
                        <div className="text-xs font-medium truncate">{cat.name}</div>
                        <div className="text-[10px] text-text-muted truncate">{cat.description}</div>
                      </div>
                    </div>
                    {cat.badge && (
                      <span className={`text-[10px] font-mono px-1.5 py-0.5 rounded shrink-0 ml-1.5 ${cat.badgeColor}`}>
                        {cat.badge}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>

            {/* Bottom Meta */}
            <div className="p-3 border-t border-surface-line text-[11px] text-text-muted flex items-center justify-between">
              <span className="font-mono">v0.1.0-alpha.1</span>
              <span className="inline-flex items-center gap-1 text-status-success">
                <span className="w-1.5 h-1.5 rounded-full bg-status-success inline-block animate-pulse" />
                Agent Ready
              </span>
            </div>
          </div>

          {/* Middle Configuration Canvas (Flex-1) */}
          <div className="flex-1 overflow-y-auto p-6 bg-surface-base">
            {selectedCategory === "overview" && (
              <div className="space-y-6">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">System & Appearance</h3>
                  <p className="text-xs text-text-muted mt-1">
                    Visual interface themes, developer preferences, and workspace configuration.
                  </p>
                </div>

                {/* Theme Selector */}
                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">Interface Theme</div>
                      <div className="text-[11px] text-text-muted">High-contrast dark mode is default for code clarity.</div>
                    </div>
                  </div>
                  <div className="grid grid-cols-3 gap-2">
                    {(["dark", "light", "system"] as ThemePreference[]).map((mode) => (
                      <button
                        key={mode}
                        type="button"
                        onClick={() => onThemePreferenceChange(mode)}
                        className={`p-3 rounded-lg border text-left transition-all ${
                          themePreference === mode
                            ? "border-accent-action bg-surface-overlay text-text-primary ring-1 ring-accent-action"
                            : "border-surface-line bg-surface-base text-text-muted hover:border-surface-overlay hover:text-text-secondary"
                        }`}
                      >
                        <div className="text-xs font-medium capitalize">{mode}</div>
                        <div className="text-[10px] text-text-muted mt-0.5">
                          {mode === "dark" ? "Low-strain dark palette" : mode === "light" ? "Bright contrast" : "Matches macOS"}
                        </div>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Workspace paths */}
                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">Workspace Binding</div>
                      <div className="text-[11px] text-text-muted">Sessions are bound to this project context.</div>
                    </div>
                    <span className="text-[11px] font-mono text-accent-structure">Affine Mode</span>
                  </div>
                  <div className="p-2.5 rounded bg-surface-base border border-surface-line font-mono text-xs text-text-secondary break-all">
                    /Users/lifelinelogics/RustProjects/gospel
                  </div>
                </div>
              </div>
            )}

            {selectedCategory === "autonomy" && (
              <div className="space-y-6">
                <div>
                  <div className="flex items-center justify-between">
                    <h3 className="text-base font-semibold text-text-primary m-0">Agent Autonomy Policy (ADR-0014)</h3>
                    <span className="text-[11px] font-mono px-2 py-0.5 rounded bg-accent-action/10 text-accent-structure">
                      Global Policy
                    </span>
                  </div>
                  <p className="text-xs text-text-muted mt-1">
                    Control how command tools (<code>run_shell_command</code>, <code>run_git_command</code>, <code>run_skill_script</code>) execute.
                  </p>
                </div>

                {/* Mode Switcher */}
                <div className="grid grid-cols-2 gap-3">
                  <button
                    type="button"
                    onClick={() => void autonomy.setMode("ask")}
                    className={`p-4 rounded-lg border text-left transition-all ${
                      autonomy.mode === "ask"
                        ? "border-accent-action bg-surface-elevated text-text-primary ring-1 ring-accent-action"
                        : "border-surface-line bg-surface-sunken/40 text-text-muted hover:border-surface-overlay hover:text-text-secondary"
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1">
                      <div className="text-xs font-semibold text-text-primary">Ask Mode (Default)</div>
                      {autonomy.mode === "ask" && <CheckCircle2 size={14} className="text-accent-structure" />}
                    </div>
                    <p className="text-[11px] text-text-muted m-0">
                      Prompts for mutating commands. Blocks dangerous restricted commands (recursive rm, shell interpreters).
                    </p>
                  </button>

                  <button
                    type="button"
                    onClick={() => void autonomy.setMode("auto")}
                    className={`p-4 rounded-lg border text-left transition-all ${
                      autonomy.mode === "auto"
                        ? "border-accent-signal bg-surface-elevated text-text-primary ring-1 ring-accent-signal"
                        : "border-surface-line bg-surface-sunken/40 text-text-muted hover:border-surface-overlay hover:text-text-secondary"
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1">
                      <div className="text-xs font-semibold text-accent-signal">Auto Mode</div>
                      {autonomy.mode === "auto" && <AlertTriangle size={14} className="text-accent-signal" />}
                    </div>
                    <p className="text-[11px] text-text-muted m-0">
                      Zero human prompts. Runs commands autonomously except those matching explicit User Deny Rules.
                    </p>
                  </button>
                </div>

                {/* Reject / Deny Patterns */}
                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">Deny & Reject Patterns</div>
                      <div className="text-[11px] text-text-muted">Commands matching these patterns are blocked or prompt even in Auto mode.</div>
                    </div>
                    <span className="text-[10px] font-mono text-text-muted">
                      {autonomy.policy.reject_patterns.length} Rules Active
                    </span>
                  </div>

                  <div className="space-y-1.5">
                    {autonomy.policy.reject_patterns.map((rule) => (
                      <div
                        key={rule}
                        className="flex items-center justify-between px-3 py-1.5 rounded bg-surface-base border border-surface-line font-mono text-xs text-text-secondary"
                      >
                        <span className="text-status-error">{rule}</span>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => void autonomy.removeRule(rule)}
                          className="h-6 px-1.5 text-[10px] text-text-muted hover:text-status-error"
                        >
                          Remove
                        </Button>
                      </div>
                    ))}
                  </div>

                  <div className="flex gap-2 pt-1">
                    <input
                      type="text"
                      placeholder="Add pattern (e.g. drop table*, curl *)..."
                      value={newRule}
                      onChange={(e) => setNewRule(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && newRule.trim()) {
                          void autonomy.addRejectRule(newRule.trim());
                          setNewRule("");
                        }
                      }}
                      className="flex-1 px-3 py-1.5 text-xs bg-surface-base border border-surface-line rounded text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent-action font-mono"
                    />
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        if (newRule.trim()) {
                          void autonomy.addRejectRule(newRule.trim());
                          setNewRule("");
                        }
                      }}
                      className="text-xs h-8 border-surface-line text-text-secondary"
                    >
                      Add Deny Rule
                    </Button>
                  </div>
                </div>

                {/* Loop Guards Info */}
                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-2">
                  <div className="text-xs font-semibold text-text-primary">Turn Loop Guards & Verification</div>
                  <div className="grid grid-cols-2 gap-3 text-xs text-text-muted">
                    <div className="p-2.5 rounded bg-surface-base border border-surface-line">
                      <span className="font-semibold text-text-secondary">Loop Warning:</span> 3 identical tool calls
                    </div>
                    <div className="p-2.5 rounded bg-surface-base border border-surface-line">
                      <span className="font-semibold text-text-secondary">Controlled Stop:</span> 5 calls (Prevents runaways)
                    </div>
                  </div>
                </div>
              </div>
            )}

            {selectedCategory === "providers" && (
              <div className="space-y-6">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">Credentialed Providers</h3>
                  <p className="text-xs text-text-muted mt-1">
                    Manage model provider credentials, OAuth sessions, and visibility.
                  </p>
                </div>

                <div className="space-y-3">
                  {providers.map((p) => (
                    <div
                      key={p.id}
                      className="p-4 rounded-lg bg-surface-elevated border border-surface-line flex items-center justify-between"
                    >
                      <div className="flex items-center gap-3">
                        <div className="w-8 h-8 rounded-md bg-surface-overlay flex items-center justify-center font-bold text-xs text-text-primary">
                          {p.name.slice(0, 2).toUpperCase()}
                        </div>
                        <div>
                          <div className="flex items-center gap-2">
                            <span className="text-xs font-semibold text-text-primary">{p.name}</span>
                            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-surface-overlay text-text-muted">
                              {p.authType}
                            </span>
                          </div>
                          <div className="text-[11px] text-text-muted">
                            {p.credentialed
                              ? `${p.modelCount} models available`
                              : "No credentials detected in ~/.config/gospel"}
                          </div>
                        </div>
                      </div>

                      <div className="flex items-center gap-2">
                        {p.credentialed ? (
                          <span className="flex items-center gap-1 text-[11px] text-status-success font-medium">
                            <CheckCircle2 size={13} /> Connected
                          </span>
                        ) : (
                          <Button variant="outline" size="sm" className="h-7 text-xs border-surface-line">
                            <Key size={12} className="mr-1" /> Configure
                          </Button>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {selectedCategory === "mcp" && (
              <div className="space-y-6">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">Model Context Protocol (MCP)</h3>
                  <p className="text-xs text-text-muted mt-1">
                    Connect local MCP servers to furnish the agent with dynamic filesystem, git, or API tools.
                  </p>
                </div>

                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">Active MCP Servers</div>
                      <div className="text-[11px] text-text-muted">Tools from connected servers are exposed to the main agent.</div>
                    </div>
                    <Button variant="outline" size="sm" className="h-7 text-xs border-surface-line">
                      + Add Server
                    </Button>
                  </div>

                  <div className="space-y-2">
                    <div className="p-3 rounded bg-surface-base border border-surface-line flex items-center justify-between text-xs">
                      <div className="flex items-center gap-2">
                        <span className="w-2 h-2 rounded-full bg-status-success" />
                        <span className="font-semibold text-text-primary">git-context</span>
                        <span className="text-text-muted text-[11px]">(stdio: bun run git-mcp)</span>
                      </div>
                      <span className="text-[11px] font-mono text-status-success">6 Tools</span>
                    </div>

                    <div className="p-3 rounded bg-surface-base border border-surface-line flex items-center justify-between text-xs">
                      <div className="flex items-center gap-2">
                        <span className="w-2 h-2 rounded-full bg-status-success" />
                        <span className="font-semibold text-text-primary">filesystem-inspect</span>
                        <span className="text-text-muted text-[11px]">(stdio: npx -y @modelcontextprotocol/server-filesystem)</span>
                      </div>
                      <span className="text-[11px] font-mono text-status-success">4 Tools</span>
                    </div>
                  </div>
                </div>
              </div>
            )}

            {selectedCategory === "skills" && (
              <div className="space-y-6">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">Discovered Skills & Prompts</h3>
                  <p className="text-xs text-text-muted mt-1">
                    Skills steer agent behavior and inject instructions on matching prompts.
                  </p>
                </div>

                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="text-xs font-semibold text-text-primary">Discovery Locations</div>
                  <div className="space-y-2 font-mono text-xs">
                    <div className="p-2 rounded bg-surface-base border border-surface-line text-text-secondary flex justify-between">
                      <span>Workspace: .agents/skills/</span>
                      <span className="text-accent-structure">Precedence 1</span>
                    </div>
                    <div className="p-2 rounded bg-surface-base border border-surface-line text-text-secondary flex justify-between">
                      <span>Global: ~/.agents/skills/</span>
                      <span className="text-text-muted">Precedence 2</span>
                    </div>
                  </div>
                </div>
              </div>
            )}

            {selectedCategory === "data" && (
              <div className="space-y-6">
                <div>
                  <h3 className="text-base font-semibold text-text-primary m-0">Storage, Retention & Traces</h3>
                  <p className="text-xs text-text-muted mt-1">
                    Manage SQLite session store, Display Transcripts, and capped Trace Logs.
                  </p>
                </div>

                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xs font-semibold text-text-primary">Session Archive & Cleanup</div>
                      <div className="text-[11px] text-text-muted">
                        {archiveStats
                          ? `${archiveStats.archived_count} archived, ${archiveStats.expired_count} expired, ${Math.round(archiveStats.archived_bytes / 1024)} KB stored`
                          : "14 sessions stored"}
                      </div>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void onRunArchiveMaintenance?.()}
                      disabled={archivePolicySaving}
                      className="text-xs h-8 border-surface-line"
                    >
                      Run Cleanup
                    </Button>
                  </div>

                  <div className="text-xs text-text-muted pt-2 border-t border-surface-line">
                    Trace log files are capped at <strong>250 MB</strong> with <strong>30-day retention</strong> in application storage.
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Right Live Agent Impact Inspector Sidecar (290px) */}
          <div className="w-[290px] flex flex-col bg-surface-sunken/40 shrink-0 p-4 border-l border-surface-line space-y-4 overflow-y-auto">
            <div className="flex items-center gap-2 pb-2 border-b border-surface-line">
              <Sparkles size={14} className="text-accent-structure" />
              <span className="text-xs font-semibold text-text-primary">Agent Impact Inspector</span>
            </div>

            <div className="space-y-3">
              <div>
                <div className="text-[11px] font-medium text-text-secondary">{agentImpactPreview.title}</div>
                <p className="text-[11px] text-text-muted mt-1 leading-relaxed">
                  {agentImpactPreview.summary}
                </p>
              </div>

              <div className="p-3 rounded-lg bg-surface-base border border-surface-line space-y-1.5">
                <div className="text-[10px] font-mono uppercase tracking-wider text-accent-structure flex items-center justify-between">
                  <span>Preamble Injection</span>
                  <Terminal size={11} />
                </div>
                <pre className="font-mono text-[10px] text-text-secondary whitespace-pre-wrap leading-tight bg-surface-sunken p-2 rounded border border-surface-line">
                  {agentImpactPreview.preambleSnippet}
                </pre>
              </div>

              <div className="p-2.5 rounded bg-surface-elevated/70 border border-surface-line">
                <div className="text-[10px] text-text-muted uppercase tracking-wider">Turn Overhead</div>
                <div className="text-xs font-mono font-medium text-text-primary mt-0.5">
                  {agentImpactPreview.tokenImpact}
                </div>
              </div>
            </div>

            <div className="mt-auto pt-4 border-t border-surface-line text-[10px] text-text-muted flex items-center gap-1.5">
              <Info size={12} className="shrink-0" />
              Changes update the live agent harness immediately for the next turn.
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
