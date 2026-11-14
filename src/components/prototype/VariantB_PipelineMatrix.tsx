import { useState, useMemo } from "react";
import {
  Activity,
  ShieldCheck,
  Server,
  Cpu,
  Check,
  X,
  Code2,
  RefreshCw,
  HardDrive,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ProviderConfig } from "../ProviderSelector";
import type { ArchivePolicy, ArchiveStats, ThemePreference } from "../../types";
import { useAutonomyPolicy } from "../../hooks/useAutonomyPolicy";

interface VariantBProps {
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

type PipelineNodeId = "all" | "auth" | "routing" | "tools" | "guardrails" | "storage";

export function VariantB_PipelineMatrix({
  onClose,
  providers,
  onRefreshAvailability,
  isRefreshingModels,
  themePreference,
  onThemePreferenceChange,
  archiveStats,
  activeWorkspaceName = "Current Workspace",
  onRunArchiveMaintenance,
}: VariantBProps) {
  const [selectedNode, setSelectedNode] = useState<PipelineNodeId>("all");
  const [showRawConfig, setShowRawConfig] = useState(false);
  const [pingStatus, setPingStatus] = useState<Record<string, number | "testing">>({});
  const autonomy = useAutonomyPolicy();

  const credentialedCount = providers.filter((p) => p.credentialed).length;

  const testPing = (providerId: string) => {
    setPingStatus((prev) => ({ ...prev, [providerId]: "testing" }));
    setTimeout(() => {
      const simulatedLatency = Math.floor(Math.random() * 120) + 45;
      setPingStatus((prev) => ({ ...prev, [providerId]: simulatedLatency }));
    }, 450);
  };

  const pipelineNodes = [
    {
      id: "auth" as PipelineNodeId,
      step: "01",
      name: "Credentials",
      metric: `${credentialedCount}/${providers.length} OK`,
      status: credentialedCount > 0 ? "healthy" : "warning",
      icon: KeyIcon,
    },
    {
      id: "routing" as PipelineNodeId,
      step: "02",
      name: "Inference Router",
      metric: "Sub-50ms Dispatch",
      status: "healthy",
      icon: Cpu,
    },
    {
      id: "tools" as PipelineNodeId,
      step: "03",
      name: "Live Tools & MCP",
      metric: "18 Tools Active",
      status: "healthy",
      icon: Server,
    },
    {
      id: "guardrails" as PipelineNodeId,
      step: "04",
      name: "Autonomy Policy",
      metric: autonomy.mode === "auto" ? "Autonomous" : "Ask Required",
      status: autonomy.mode === "auto" ? "info" : "healthy",
      icon: ShieldCheck,
    },
    {
      id: "storage" as PipelineNodeId,
      step: "05",
      name: "SQLite & Traces",
      metric: archiveStats ? `${archiveStats.archived_count} records` : "Healthy",
      status: "healthy",
      icon: HardDrive,
    },
  ];

  const rawConfigJson = useMemo(() => {
    return JSON.stringify(
      {
        version: "1.0",
        workspace: activeWorkspaceName,
        autonomy: {
          mode: autonomy.mode,
          reject_patterns: autonomy.policy.reject_patterns,
          allowed_patterns: autonomy.policy.allowed_patterns,
        },
        providers: providers.map((p) => ({
          id: p.id,
          name: p.name,
          authType: p.authType,
          credentialed: p.credentialed,
          visible: p.visible,
          modelCount: p.modelCount,
        })),
        theme: themePreference,
        trace_storage_mb: 14.2,
      },
      null,
      2
    );
  }, [activeWorkspaceName, autonomy.mode, autonomy.policy, providers, themePreference]);

  return (
    <div className="fixed inset-0 bg-scrim z-[var(--z-dialog)] flex items-center justify-center p-4 animate-fade-in font-sans">
      <div
        className="relative w-full max-w-[1100px] h-[88vh] max-h-[800px] flex flex-col bg-[#0f1015] border border-[#2b2d3a] rounded-xl shadow-[0_30px_70px_rgba(0,0,0,0.6)] overflow-hidden animate-slide-up"
        role="dialog"
        aria-modal="true"
        aria-labelledby="matrix-title"
      >
        {/* HUD Top Bar */}
        <div className="flex items-center justify-between px-6 py-3 border-b border-[#232532] bg-[#090a0f] shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-accent-action/15 border border-accent-action/30 text-accent-structure font-mono text-xs">
              <Activity size={13} className="text-accent-structure animate-pulse" />
              <span>COMMAND & CONTROL MATRIX</span>
            </div>
            <span className="text-xs text-text-muted">|</span>
            <span className="text-xs text-text-secondary font-mono">{activeWorkspaceName}</span>
          </div>

          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => setShowRawConfig(!showRawConfig)}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-xs font-mono transition-colors border ${
                showRawConfig
                  ? "bg-accent-action text-white border-accent-action"
                  : "bg-[#181922] text-text-secondary border-[#2f3140] hover:text-white"
              }`}
            >
              <Code2 size={13} />
              {showRawConfig ? "GUI Matrix" : "Raw JSON Config"}
            </button>

            <Button
              variant="outline"
              size="sm"
              onClick={() => void onRefreshAvailability(true)}
              disabled={isRefreshingModels}
              className="h-7 text-xs border-[#2f3140] bg-[#181922] text-text-secondary hover:text-white"
            >
              <RefreshCw size={12} className={`mr-1.5 ${isRefreshingModels ? "animate-spin" : ""}`} />
              Ping All
            </Button>

            <button
              type="button"
              onClick={onClose}
              className="p-1 rounded text-text-muted hover:text-white hover:bg-[#20222e]"
              aria-label="Close"
            >
              <X size={16} />
            </button>
          </div>
        </div>

        {/* Visual Pipeline Ribbon */}
        <div className="px-6 py-3.5 border-b border-[#232532] bg-[#12131b] shrink-0">
          <div className="text-[10px] font-mono uppercase tracking-wider text-text-muted mb-2 flex items-center justify-between">
            <span>Agent Execution Pipeline Flow</span>
            <span className="text-text-muted">Click node to inspect tier</span>
          </div>

          <div className="grid grid-cols-5 gap-2">
            {pipelineNodes.map((node) => {
              const isSelected = selectedNode === node.id || selectedNode === "all";
              const isStrictActive = selectedNode === node.id;
              const Icon = node.icon;
              return (
                <button
                  key={node.id}
                  type="button"
                  onClick={() => setSelectedNode(selectedNode === node.id ? "all" : node.id)}
                  className={`relative p-2.5 rounded-lg border text-left transition-all group ${
                    isStrictActive
                      ? "bg-surface-elevated border-accent-action ring-1 ring-accent-action"
                      : isSelected
                        ? "bg-[#161822] border-[#292c3d] hover:border-[#3d425a]"
                        : "bg-[#12131b] border-transparent opacity-40 hover:opacity-80"
                  }`}
                >
                  <div className="flex items-center justify-between text-[10px] font-mono text-text-muted mb-1">
                    <span>STEP {node.step}</span>
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${
                        node.status === "healthy"
                          ? "bg-status-success"
                          : node.status === "warning"
                            ? "bg-status-warning"
                            : "bg-accent-structure"
                      }`}
                    />
                  </div>
                  <div className="text-xs font-semibold text-text-primary flex items-center gap-1.5">
                    <Icon size={13} className="text-text-muted group-hover:text-accent-structure" />
                    <span className="truncate">{node.name}</span>
                  </div>
                  <div className="text-[11px] font-mono text-text-secondary mt-0.5 truncate">
                    {node.metric}
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Matrix Main Viewport */}
        <div className="flex-1 overflow-y-auto p-6 bg-[#0c0d12]">
          {showRawConfig ? (
            <div className="h-full flex flex-col space-y-2">
              <div className="flex items-center justify-between text-xs text-text-muted font-mono">
                <span>Config file path: ~/.config/gospel/credentials.json</span>
                <span>Mode: Read-only live projection</span>
              </div>
              <pre className="flex-1 p-4 bg-[#08090d] border border-[#232532] rounded-lg font-mono text-xs text-accent-structure overflow-auto leading-relaxed">
                {rawConfigJson}
              </pre>
            </div>
          ) : (
            <div className="space-y-6">
              {/* TIER 1: PROVIDERS & LATENCIES MATRIX */}
              {(selectedNode === "all" || selectedNode === "auth" || selectedNode === "routing") && (
                <section className="space-y-2.5">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Cpu size={15} className="text-accent-structure" />
                      <h3 className="text-xs font-semibold uppercase tracking-wider text-text-primary m-0">
                        Providers, Auth & Latency Benchmarks
                      </h3>
                    </div>
                    <span className="text-[11px] font-mono text-text-muted">
                      {credentialedCount} of {providers.length} Ready
                    </span>
                  </div>

                  <div className="border border-[#232532] rounded-lg overflow-hidden bg-[#13151e]">
                    <table className="w-full text-left text-xs">
                      <thead className="bg-[#0b0c11] text-[10px] font-mono uppercase text-text-muted border-b border-[#232532]">
                        <tr>
                          <th className="py-2.5 px-4 font-medium">Provider</th>
                          <th className="py-2.5 px-4 font-medium">Auth Kind</th>
                          <th className="py-2.5 px-4 font-medium">Models</th>
                          <th className="py-2.5 px-4 font-medium">Status</th>
                          <th className="py-2.5 px-4 font-medium">Live Latency</th>
                          <th className="py-2.5 px-4 font-medium text-right">Actions</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-[#1e202d] text-text-secondary">
                        {providers.map((p) => {
                          const ping = pingStatus[p.id];
                          return (
                            <tr key={p.id} className="hover:bg-[#181a26] transition-colors">
                              <td className="py-2.5 px-4 font-medium text-text-primary flex items-center gap-2">
                                <span className="w-2 h-2 rounded-full bg-surface-overlay" />
                                {p.name}
                              </td>
                              <td className="py-2.5 px-4 font-mono text-[11px] text-text-muted">
                                {p.authType}
                              </td>
                              <td className="py-2.5 px-4 font-mono text-[11px]">
                                {p.modelCount} models
                              </td>
                              <td className="py-2.5 px-4">
                                {p.credentialed ? (
                                  <span className="inline-flex items-center gap-1 text-[11px] font-mono text-status-success">
                                    <Check size={12} /> Ready
                                  </span>
                                ) : (
                                  <span className="inline-flex items-center gap-1 text-[11px] font-mono text-status-warning">
                                    <X size={12} /> Missing Key
                                  </span>
                                )}
                              </td>
                              <td className="py-2.5 px-4 font-mono text-[11px]">
                                {ping === "testing" ? (
                                  <span className="text-text-muted animate-pulse">Pinging...</span>
                                ) : typeof ping === "number" ? (
                                  <span className={ping < 100 ? "text-status-success" : "text-status-warning"}>
                                    {ping} ms
                                  </span>
                                ) : (
                                  <span className="text-text-muted">-</span>
                                )}
                              </td>
                              <td className="py-2.5 px-4 text-right">
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => testPing(p.id)}
                                  className="h-6 px-2 text-[10px] font-mono text-text-muted hover:text-text-primary"
                                >
                                  Ping
                                </Button>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                </section>
              )}

              {/* TIER 2: AUTONOMY & COMMAND PERMISSIONS MATRIX */}
              {(selectedNode === "all" || selectedNode === "guardrails") && (
                <section className="space-y-2.5">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <ShieldCheck size={15} className="text-accent-structure" />
                      <h3 className="text-xs font-semibold uppercase tracking-wider text-text-primary m-0">
                        Shell & Tool Autonomy Matrix
                      </h3>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-[11px] text-text-muted">Active Mode:</span>
                      <button
                        type="button"
                        onClick={() => void autonomy.setMode(autonomy.mode === "ask" ? "auto" : "ask")}
                        className={`px-2.5 py-0.5 rounded text-[11px] font-mono font-semibold uppercase border transition-colors ${
                          autonomy.mode === "auto"
                            ? "bg-accent-signal/20 text-accent-signal border-accent-signal/40"
                            : "bg-accent-action/20 text-accent-structure border-accent-action/40"
                        }`}
                      >
                        {autonomy.mode} (Click to toggle)
                      </button>
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    {/* Denied / Blocked Table */}
                    <div className="p-3.5 rounded-lg border border-[#232532] bg-[#13151e] space-y-2">
                      <div className="flex items-center justify-between text-xs font-semibold text-text-primary">
                        <span className="text-status-error flex items-center gap-1.5">
                          <X size={13} /> Deny / Blocked Patterns
                        </span>
                        <span className="text-[10px] font-mono text-text-muted">
                          {autonomy.policy.reject_patterns.length} Rules
                        </span>
                      </div>
                      <div className="space-y-1">
                        {autonomy.policy.reject_patterns.map((rule) => (
                          <div
                            key={rule}
                            className="flex items-center justify-between px-2.5 py-1 rounded bg-[#0b0c11] border border-[#1e202d] font-mono text-[11px] text-text-secondary"
                          >
                            <span className="text-status-error">{rule}</span>
                            <span className="text-[9px] text-text-muted uppercase">Blocked</span>
                          </div>
                        ))}
                      </div>
                    </div>

                    {/* Auto-Approved Commands */}
                    <div className="p-3.5 rounded-lg border border-[#232532] bg-[#13151e] space-y-2">
                      <div className="flex items-center justify-between text-xs font-semibold text-text-primary">
                        <span className="text-status-success flex items-center gap-1.5">
                          <Check size={13} /> Workspace Pre-approved Tools
                        </span>
                        <span className="text-[10px] font-mono text-text-muted">4 Defaults</span>
                      </div>
                      <div className="space-y-1 font-mono text-[11px] text-text-secondary">
                        <div className="flex items-center justify-between px-2.5 py-1 rounded bg-[#0b0c11] border border-[#1e202d]">
                          <span className="text-text-primary">cargo test* / cargo check*</span>
                          <span className="text-[9px] text-status-success uppercase">Auto</span>
                        </div>
                        <div className="flex items-center justify-between px-2.5 py-1 rounded bg-[#0b0c11] border border-[#1e202d]">
                          <span className="text-text-primary">git status / git diff</span>
                          <span className="text-[9px] text-status-success uppercase">Auto</span>
                        </div>
                        <div className="flex items-center justify-between px-2.5 py-1 rounded bg-[#0b0c11] border border-[#1e202d]">
                          <span className="text-text-primary">bun run test*</span>
                          <span className="text-[9px] text-status-success uppercase">Auto</span>
                        </div>
                      </div>
                    </div>
                  </div>
                </section>
              )}

              {/* TIER 3: STORAGE & PERFORMANCE METRICS */}
              {(selectedNode === "all" || selectedNode === "storage") && (
                <section className="space-y-2.5">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <HardDrive size={15} className="text-accent-structure" />
                      <h3 className="text-xs font-semibold uppercase tracking-wider text-text-primary m-0">
                        Storage, Quotas & Maintenance
                      </h3>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void onRunArchiveMaintenance?.()}
                      className="h-6 text-[11px] border-[#2f3140] bg-[#181922] text-text-secondary"
                    >
                      Purge Stale Traces
                    </Button>
                  </div>

                  <div className="grid grid-cols-3 gap-3">
                    <div className="p-3 rounded-lg border border-[#232532] bg-[#13151e]">
                      <div className="text-[10px] font-mono uppercase text-text-muted">Trace Log Quota</div>
                      <div className="text-sm font-mono font-semibold text-text-primary mt-1">14.2 MB / 250 MB</div>
                      <div className="w-full bg-[#1e202d] h-1.5 rounded-full mt-2 overflow-hidden">
                        <div className="bg-accent-action h-full rounded-full" style={{ width: "6%" }} />
                      </div>
                    </div>

                    <div className="p-3 rounded-lg border border-[#232532] bg-[#13151e]">
                      <div className="text-[10px] font-mono uppercase text-text-muted">Retention Policy</div>
                      <div className="text-sm font-mono font-semibold text-text-primary mt-1">30 Days Sliding</div>
                      <div className="text-[10px] text-text-muted mt-1.5">Auto-cleans expired turns</div>
                    </div>

                    <div className="p-3 rounded-lg border border-[#232532] bg-[#13151e]">
                      <div className="text-[10px] font-mono uppercase text-text-muted">Theme Palette</div>
                      <div className="flex items-center gap-2 mt-1">
                        {(["dark", "light", "system"] as ThemePreference[]).map((mode) => (
                          <button
                            key={mode}
                            type="button"
                            onClick={() => onThemePreferenceChange(mode)}
                            className={`px-2 py-0.5 rounded text-[10px] font-mono uppercase border ${
                              themePreference === mode
                                ? "bg-accent-action text-white border-accent-action"
                                : "bg-[#181922] text-text-muted border-[#2a2c3a]"
                            }`}
                          >
                            {mode}
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                </section>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function KeyIcon(props: React.SVGProps<SVGSVGElement>) {
  return (
    <svg
      width="1em"
      height="1em"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      {...props}
    >
      <circle cx="7.5" cy="15.5" r="5.5" />
      <path d="m21 2-9.6 9.6" />
      <path d="m15.5 7.5 3 3L22 7l-3-3" />
    </svg>
  );
}
