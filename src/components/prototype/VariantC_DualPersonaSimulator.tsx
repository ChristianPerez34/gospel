import { useState, useMemo } from "react";
import {
  User,
  Bot,
  Terminal,
  Play,
  CheckCircle2,
  AlertTriangle,
  XCircle,
  Sparkles,
  Shield,
  Code,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ProviderConfig } from "../ProviderSelector";
import type { ArchivePolicy, ArchiveStats, ThemePreference } from "../../types";
import { useAutonomyPolicy } from "../../hooks/useAutonomyPolicy";

interface VariantCProps {
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

type LensMode = "developer" | "agent" | "simulator";

export function VariantC_DualPersonaSimulator({
  onClose,
  providers,
  themePreference,
  onThemePreferenceChange,
  archiveStats,
  activeWorkspaceName = "Current Project",
  onRunArchiveMaintenance,
}: VariantCProps) {
  const [activeLens, setActiveLens] = useState<LensMode>("simulator");
  const autonomy = useAutonomyPolicy();

  // Simulator state
  const [testCommand, setTestCommand] = useState("rm -rf dist");
  const [testPrompt, setTestPrompt] = useState("help me diagnose this performance bug");

  // Command safety evaluation simulator
  const evaluatedCommand = useMemo(() => {
    const cmd = testCommand.trim();
    if (!cmd) return null;

    // Check reject patterns
    for (const pattern of autonomy.policy.reject_patterns) {
      const regex = new RegExp(`^${pattern.replace(/\*/g, ".*")}$`, "i");
      if (regex.test(cmd)) {
        return {
          status: "blocked",
          reason: `Matches User Deny Rule: "${pattern}"`,
          action: autonomy.mode === "auto" ? "Prompts human approval" : "Hard Denied",
          icon: XCircle,
          color: "text-status-error bg-status-error/10 border-status-error/30",
        };
      }
    }

    // Check restricted commands
    if (
      cmd.startsWith("rm -rf /") ||
      cmd.includes(";") ||
      cmd.includes("|") ||
      cmd.startsWith("curl") ||
      cmd.startsWith("bash") ||
      cmd.startsWith("sh")
    ) {
      return {
        status: "restricted",
        reason: "Restricted command pattern (metacharacters or destructive root)",
        action: autonomy.mode === "auto" ? "Runs (Auto mode active)" : "Hard Denied in Ask mode",
        icon: AlertTriangle,
        color: "text-accent-signal bg-accent-signal/10 border-accent-signal/30",
      };
    }

    // Check allow patterns
    for (const pattern of autonomy.policy.allowed_patterns) {
      const regex = new RegExp(`^${pattern.replace(/\*/g, ".*")}$`, "i");
      if (regex.test(cmd)) {
        return {
          status: "allowed",
          reason: `Matches Pre-approved Rule: "${pattern}"`,
          action: "Runs immediately without prompting",
          icon: CheckCircle2,
          color: "text-status-success bg-status-success/10 border-status-success/30",
        };
      }
    }

    return {
      status: autonomy.mode === "auto" ? "auto" : "ask",
      reason: "Standard workspace mutating command",
      action: autonomy.mode === "auto" ? "Runs without prompting" : "Prompts user for one-time confirmation",
      icon: autonomy.mode === "auto" ? CheckCircle2 : Shield,
      color: autonomy.mode === "auto" ? "text-status-success bg-status-success/10 border-status-success/30" : "text-accent-structure bg-accent-structure/10 border-accent-structure/30",
    };
  }, [testCommand, autonomy.policy, autonomy.mode]);

  // Skill match evaluator simulator
  const simulatedMatchedSkills = useMemo(() => {
    const p = testPrompt.toLowerCase();
    const matches: Array<{ name: string; score: number; source: string }> = [];

    if (p.includes("diagnose") || p.includes("bug") || p.includes("perf")) {
      matches.push({ name: "diagnose", score: 0.89, source: "Workspace (.agents/skills)" });
    }
    if (p.includes("prototype") || p.includes("design") || p.includes("page")) {
      matches.push({ name: "prototype", score: 0.94, source: "Workspace (.agents/skills)" });
    }
    if (p.includes("test") || p.includes("tdd")) {
      matches.push({ name: "tdd", score: 0.82, source: "Workspace (.agents/skills)" });
    }
    if (p.includes("clean") || p.includes("simple") || p.includes("minimal")) {
      matches.push({ name: "ponytail", score: 0.78, source: "Global (~/.agents/skills)" });
    }

    return matches;
  }, [testPrompt]);

  return (
    <div className="fixed inset-0 bg-scrim z-[var(--z-dialog)] flex items-center justify-center p-4 animate-fade-in font-sans">
      <div
        className="relative w-full max-w-[1020px] h-[86vh] max-h-[780px] flex flex-col bg-surface-base border border-surface-overlay rounded-xl shadow-[0_24px_60px_rgba(0,0,0,0.5)] overflow-hidden animate-slide-up"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dual-persona-title"
      >
        {/* Header with 3-Way Lens Switcher */}
        <div className="flex items-center justify-between px-6 py-3.5 border-b border-surface-line bg-surface-sunken shrink-0">
          <div>
            <h2 id="dual-persona-title" className="text-body font-semibold text-text-primary m-0">
              Gospel Configuration & Harness Lens
            </h2>
            <p className="text-[11px] text-text-muted m-0">
              Switch perspective between human ergonomics, agent safety rules, and live sandbox testing.
            </p>
          </div>

          <div className="flex items-center gap-2">
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

        {/* Lens Switcher Tabs */}
        <div className="px-6 py-2.5 bg-surface-sunken/80 border-b border-surface-line flex items-center justify-between shrink-0">
          <div className="inline-flex p-1 rounded-lg bg-surface-base border border-surface-line gap-1">
            <button
              type="button"
              onClick={() => setActiveLens("developer")}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                activeLens === "developer"
                  ? "bg-surface-elevated text-text-primary shadow-sm border border-surface-line"
                  : "text-text-muted hover:text-text-secondary"
              }`}
            >
              <User size={13} className={activeLens === "developer" ? "text-accent-structure" : ""} />
              Developer Environment
            </button>

            <button
              type="button"
              onClick={() => setActiveLens("agent")}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                activeLens === "agent"
                  ? "bg-surface-elevated text-text-primary shadow-sm border border-surface-line"
                  : "text-text-muted hover:text-text-secondary"
              }`}
            >
              <Bot size={13} className={activeLens === "agent" ? "text-accent-action" : ""} />
              Agent Harness & Safety
            </button>

            <button
              type="button"
              onClick={() => setActiveLens("simulator")}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                activeLens === "simulator"
                  ? "bg-surface-elevated text-text-primary shadow-sm border border-surface-line ring-1 ring-accent-action"
                  : "text-text-muted hover:text-text-secondary"
              }`}
            >
              <Play size={13} className={activeLens === "simulator" ? "text-accent-signal" : ""} />
              Harness Sandbox Simulator
            </button>
          </div>

          <span className="text-[11px] font-mono text-text-muted">
            Lens: <strong className="text-text-secondary uppercase">{activeLens}</strong>
          </span>
        </div>

        {/* Main Body */}
        <div className="flex-1 overflow-y-auto p-6 bg-surface-base">
          {/* LENS 1: DEVELOPER ENVIRONMENT */}
          {activeLens === "developer" && (
            <div className="max-w-2xl mx-auto space-y-6">
              <div className="space-y-1">
                <h3 className="text-sm font-semibold text-text-primary">Human Ergonomics & Interface</h3>
                <p className="text-xs text-text-muted">Personal display, workspace bindings, and API keys.</p>
              </div>

              {/* Theme card */}
              <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                <div className="text-xs font-semibold text-text-primary">Interface Theme</div>
                <div className="grid grid-cols-3 gap-2">
                  {(["dark", "light", "system"] as ThemePreference[]).map((mode) => (
                    <button
                      key={mode}
                      type="button"
                      onClick={() => onThemePreferenceChange(mode)}
                      className={`p-3 rounded-lg border text-left transition-all ${
                        themePreference === mode
                          ? "border-accent-action bg-surface-overlay text-text-primary ring-1 ring-accent-action"
                          : "border-surface-line bg-surface-base text-text-muted hover:text-text-secondary"
                      }`}
                    >
                      <div className="text-xs font-medium capitalize">{mode}</div>
                      <div className="text-[10px] text-text-muted mt-0.5">
                        {mode === "dark" ? "Low-fatigue dark" : mode === "light" ? "Crisp light" : "OS Synchronized"}
                      </div>
                    </button>
                  ))}
                </div>
              </div>

              {/* Workspace Context */}
              <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-2">
                <div className="flex items-center justify-between">
                  <div className="text-xs font-semibold text-text-primary">Current Workspace Folder</div>
                  <span className="text-[10px] font-mono text-status-success">
                    {providers.filter((p) => p.credentialed).length} Providers Active
                  </span>
                </div>
                <div className="p-2.5 rounded bg-surface-base border border-surface-line font-mono text-xs text-text-secondary">
                  {activeWorkspaceName}
                </div>
              </div>

              {/* Local Storage & Cache */}
              <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line flex items-center justify-between">
                <div>
                  <div className="text-xs font-semibold text-text-primary">History & Transcripts Database</div>
                  <div className="text-[11px] text-text-muted">
                    {archiveStats
                      ? `${archiveStats.archived_count} archived turns stored in SQLite`
                      : "Local conversation storage"}
                  </div>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void onRunArchiveMaintenance?.()}
                  className="h-8 text-xs border-surface-line"
                >
                  Run Maintenance
                </Button>
              </div>
            </div>
          )}

          {/* LENS 2: AGENT HARNESS & SAFETY */}
          {activeLens === "agent" && (
            <div className="max-w-2xl mx-auto space-y-6">
              <div className="space-y-1">
                <h3 className="text-sm font-semibold text-text-primary">Agent Boundaries & Guardrails</h3>
                <p className="text-xs text-text-muted">
                  Configure what the agent is permitted to execute, how it recovers from errors, and loop limits.
                </p>
              </div>

              {/* Autonomy Mode */}
              <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-xs font-semibold text-text-primary">Global Autonomy Mode (ADR-0014)</div>
                    <div className="text-[11px] text-text-muted">
                      Controls whether mutating terminal, git, and file commands prompt for approval.
                    </div>
                  </div>
                  <span className="text-[11px] font-mono px-2 py-0.5 rounded bg-surface-overlay text-accent-structure">
                    {autonomy.mode.toUpperCase()}
                  </span>
                </div>

                <div className="grid grid-cols-2 gap-3 pt-1">
                  <button
                    type="button"
                    onClick={() => void autonomy.setMode("ask")}
                    className={`p-3 rounded-lg border text-left transition-all ${
                      autonomy.mode === "ask"
                        ? "border-accent-action bg-surface-base text-text-primary ring-1 ring-accent-action"
                        : "border-surface-line bg-surface-sunken/40 text-text-muted hover:text-text-secondary"
                    }`}
                  >
                    <div className="text-xs font-semibold text-text-primary">Ask Mode</div>
                    <div className="text-[10px] text-text-muted mt-1">
                      Prompts for mutating actions. Blocks restricted shell interpreters.
                    </div>
                  </button>

                  <button
                    type="button"
                    onClick={() => void autonomy.setMode("auto")}
                    className={`p-3 rounded-lg border text-left transition-all ${
                      autonomy.mode === "auto"
                        ? "border-accent-signal bg-surface-base text-text-primary ring-1 ring-accent-signal"
                        : "border-surface-line bg-surface-sunken/40 text-text-muted hover:text-text-secondary"
                    }`}
                  >
                    <div className="text-xs font-semibold text-accent-signal">Auto Mode</div>
                    <div className="text-[10px] text-text-muted mt-1">
                      Fully autonomous execution. Only User Deny Rules will prompt.
                    </div>
                  </button>
                </div>
              </div>

              {/* Loop Guards & Verification */}
              <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                <div className="text-xs font-semibold text-text-primary">Turn Safeguards & Verification Agent</div>
                <div className="space-y-2 text-xs text-text-muted">
                  <div className="flex items-center justify-between p-2 rounded bg-surface-base border border-surface-line">
                    <span>Identical Tool Call Warning</span>
                    <span className="font-mono text-accent-structure">3 identical calls</span>
                  </div>
                  <div className="flex items-center justify-between p-2 rounded bg-surface-base border border-surface-line">
                    <span>Controlled Stop (Loop circuit breaker)</span>
                    <span className="font-mono text-status-error">5 calls (prevents token runaway)</span>
                  </div>
                  <div className="flex items-center justify-between p-2 rounded bg-surface-base border border-surface-line">
                    <span>Async Verification Sub-agent</span>
                    <span className="font-mono text-status-success">Enabled on high-risk code changes</span>
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* LENS 3: HARNESS SANDBOX SIMULATOR */}
          {activeLens === "simulator" && (
            <div className="grid grid-cols-2 gap-6 h-full">
              {/* Left Column: Command & Prompt Testers */}
              <div className="space-y-6">
                <div>
                  <h3 className="text-sm font-semibold text-text-primary m-0">Interactive Safety Simulator</h3>
                  <p className="text-xs text-text-muted mt-1">
                    Simulate how the Gospel policy engine reacts to commands and prompts in real time.
                  </p>
                </div>

                {/* Command Tester */}
                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="text-xs font-semibold text-text-primary flex items-center gap-1.5">
                      <Terminal size={14} className="text-accent-structure" />
                      Test Shell Command Classifier
                    </div>
                    <span className="text-[10px] font-mono text-text-muted">Live Evaluation</span>
                  </div>

                  <input
                    type="text"
                    value={testCommand}
                    onChange={(e) => setTestCommand(e.target.value)}
                    placeholder="Type command e.g. git push --force or cargo test..."
                    className="w-full px-3 py-2 text-xs bg-surface-base border border-surface-line rounded font-mono text-text-primary focus:outline-none focus:border-accent-action"
                  />

                  {evaluatedCommand && (
                    <div className={`p-3 rounded-lg border ${evaluatedCommand.color} space-y-1`}>
                      <div className="flex items-center gap-1.5 font-semibold text-xs">
                        <evaluatedCommand.icon size={14} />
                        <span>{evaluatedCommand.action}</span>
                      </div>
                      <div className="text-[11px] opacity-90 pl-5">
                        {evaluatedCommand.reason}
                      </div>
                    </div>
                  )}

                  <div className="text-[10px] text-text-muted">
                    Try typing: <code>git status</code>, <code>rm -rf /</code>, <code>cargo test</code>, or <code>git push --force</code>.
                  </div>
                </div>

                {/* Prompt Matcher Tester */}
                <div className="p-4 rounded-lg bg-surface-elevated border border-surface-line space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="text-xs font-semibold text-text-primary flex items-center gap-1.5">
                      <Sparkles size={14} className="text-accent-action" />
                      Test Skill Auto-Matcher
                    </div>
                    <span className="text-[10px] font-mono text-text-muted">Top-3 Scoring</span>
                  </div>

                  <input
                    type="text"
                    value={testPrompt}
                    onChange={(e) => setTestPrompt(e.target.value)}
                    placeholder="Type a sample user prompt..."
                    className="w-full px-3 py-2 text-xs bg-surface-base border border-surface-line rounded text-text-primary focus:outline-none focus:border-accent-action"
                  />

                  <div className="space-y-1.5">
                    {simulatedMatchedSkills.length > 0 ? (
                      simulatedMatchedSkills.map((match) => (
                        <div
                          key={match.name}
                          className="flex items-center justify-between p-2 rounded bg-surface-base border border-surface-line text-xs"
                        >
                          <div className="flex items-center gap-2">
                            <span className="font-mono font-semibold text-accent-structure">/{match.name}</span>
                            <span className="text-[10px] text-text-muted">({match.source})</span>
                          </div>
                          <span className="font-mono text-[11px] text-status-success">
                            {(match.score * 100).toFixed(0)}% match
                          </span>
                        </div>
                      ))
                    ) : (
                      <div className="p-2.5 rounded bg-surface-base border border-surface-line text-center text-xs text-text-muted">
                        No skills exceed match score threshold (falls back to generic general agent preamble)
                      </div>
                    )}
                  </div>
                </div>
              </div>

              {/* Right Column: Live Compiled Agent Preamble Output */}
              <div className="flex flex-col h-full bg-surface-sunken/60 rounded-lg border border-surface-line p-4 space-y-2">
                <div className="flex items-center justify-between pb-2 border-b border-surface-line">
                  <div className="flex items-center gap-2 text-xs font-semibold text-text-primary">
                    <Code size={14} className="text-accent-structure" />
                    <span>Compiled Agent Preamble (Live Preview)</span>
                  </div>
                  <span className="text-[10px] font-mono text-text-muted">
                    Token Est: ~640
                  </span>
                </div>

                <div className="flex-1 overflow-auto bg-surface-base p-3 rounded border border-surface-line font-mono text-[11px] text-text-secondary leading-relaxed whitespace-pre-wrap">
{`# System Preamble: Gospel Agent Harness

## Workspace Context
- Path: ${activeWorkspaceName}
- Mode: Affine Session

## Autonomy Policy (ADR-0014)
- Execution Mode: ${autonomy.mode.toUpperCase()}
- Denied Command Patterns: ${autonomy.policy.reject_patterns.join(", ")}
- Pre-approved Patterns: ${autonomy.policy.allowed_patterns.join(", ") || "cargo test*, git status*"}

## Active Matched Skills
${
  simulatedMatchedSkills.length > 0
    ? simulatedMatchedSkills.map((m) => `- /${m.name} (${(m.score * 100).toFixed(0)}% confidence)`).join("\n")
    : "- None (General purpose assistant mode)"
}

## Registered Tool Capabilities
- live_workspace: read_file, source_edit, search_files
- shell_execution: run_shell_command, run_git_command (Gated by Policy)
- mcp_tools: git-context, filesystem-inspect`}
                </div>

                <div className="text-[10px] text-text-muted pt-1 flex items-center justify-between">
                  <span>Injected before first token stream</span>
                  <span className="text-status-success flex items-center gap-1">
                    <CheckCircle2 size={11} /> Ready for Inference
                  </span>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
