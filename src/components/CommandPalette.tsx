import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { ModelOption, Session, Workspace } from "../types";
import { useFocusTrap } from "../hooks/useFocusTrap";

type CommandGroup = "Sessions" | "Files / context" | "Settings" | "Commands";

interface PaletteResult {
  id: string;
  group: CommandGroup;
  icon: string;
  label: string;
  detail?: string;
  shortcut?: string;
  keywords: string;
  action: () => void;
}

interface CommandPaletteProps {
  open: boolean;
  sessions: Session[];
  activeSessionId?: string | null;
  workspace?: Workspace | null;
  models: ModelOption[];
  selectedModelId: string;
  onClose: () => void;
  onSelectSession: (session: Session) => void;
  onNewSession: () => void;
  onOpenSettings: (tab?: "general" | "models") => void;
  onOpenWorkspaceSwitcher: () => void;
  onToggleSessions: () => void;
  onToggleReview: () => void;
  onSelectModel: (modelId: string) => void;
  restoreFocusRef?: RefObject<HTMLElement>;
  workspaceNames?: Record<string, string>;
}

function includesQuery(result: PaletteResult, query: string) {
  if (!query) return true;
  const haystack = `${result.label} ${result.detail ?? ""} ${result.keywords}`.toLowerCase();
  return haystack.includes(query.toLowerCase());
}

function groupResults(results: PaletteResult[]) {
  const groups: Array<{ label: CommandGroup; results: PaletteResult[] }> = [];
  for (const label of ["Sessions", "Files / context", "Settings", "Commands"] as CommandGroup[]) {
    const group = results.filter((result) => result.group === label);
    if (group.length > 0) groups.push({ label, results: group });
  }
  return groups;
}

export function CommandPalette({
  open,
  sessions,
  activeSessionId,
  workspace,
  models,
  selectedModelId,
  onClose,
  onSelectSession,
  onNewSession,
  onOpenSettings,
  onOpenWorkspaceSwitcher,
  onToggleSessions,
  onToggleReview,
  onSelectModel,
  restoreFocusRef,
  workspaceNames,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const allResults = useMemo<PaletteResult[]>(() => {
    const closeAfter = (action: () => void) => () => {
      action();
      onClose();
    };

    const sessionResults = sessions.slice(0, query ? sessions.length : 5).map((session) => {
      const workspaceName = session.workspaceId
        ? workspaceNames?.[session.workspaceId] || workspace?.name
        : workspace?.name;
      const detail = workspaceName
        ? `${session.model} · ${workspaceName} · ${session.timestamp.toLocaleString([], {
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
          })}`
        : `${session.model} · ${session.timestamp.toLocaleString([], {
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
          })}`;

      return {
        id: `session-${session.id}`,
        group: "Sessions" as const,
        icon: session.id === activeSessionId ? "A" : "S",
        label: session.title || "Untitled session",
        detail,
        keywords: `session conversation ${session.model} ${workspaceName ? workspaceName.toLowerCase() : ""}`,
        action: closeAfter(() => onSelectSession(session)),
      };
    });

    const workspaceResults: PaletteResult[] = workspace
      ? [
          {
            id: `workspace-${workspace.id}`,
            group: "Files / context",
            icon: "W",
            label: workspace.name,
            detail: workspace.path,
            shortcut: "Open",
            keywords: "workspace file context switch directory",
            action: closeAfter(onOpenWorkspaceSwitcher),
          },
        ]
      : [];

    const settingsResults: PaletteResult[] = [
      {
        id: "settings-general",
        group: "Settings",
        icon: "G",
        label: "General settings",
        detail: "Theme, shell, shortcuts",
        keywords: "settings preferences theme light dark system",
        action: closeAfter(() => onOpenSettings("general")),
      },
      {
        id: "settings-models",
        group: "Settings",
        icon: "M",
        label: "Model settings",
        detail: "Providers, credentials, availability",
        keywords: "settings models provider api key",
        action: closeAfter(() => onOpenSettings("models")),
      },
    ];

    const commandResults: PaletteResult[] = [
      {
        id: "new-session",
        group: "Commands",
        icon: "+",
        label: "New session",
        detail: "Start a clean chat thread",
        shortcut: "Cmd N",
        keywords: "new session chat",
        action: closeAfter(onNewSession),
      },
      {
        id: "toggle-sessions",
        group: "Commands",
        icon: "D",
        label: "Toggle session drawer",
        detail: "Show or hide recent sessions",
        keywords: "drawer sessions history",
        action: closeAfter(onToggleSessions),
      },
      {
        id: "toggle-review",
        group: "Commands",
        icon: "R",
        label: "Toggle review panel",
        detail: "Open or close security review",
        keywords: "security review panel findings",
        action: closeAfter(onToggleReview),
      },
      {
        id: "switch-workspace",
        group: "Commands",
        icon: "W",
        label: "Switch workspace",
        detail: "Open the workspace switcher",
        keywords: "workspace switch directory",
        action: closeAfter(onOpenWorkspaceSwitcher),
      },
      ...models.map((model) => ({
        id: `model-${model.id}`,
        group: "Commands" as const,
        icon: model.id === selectedModelId ? "A" : "M",
        label: `Use ${model.name}`,
        detail: model.provider,
        keywords: `model provider ${model.name} ${model.provider}`,
        action: closeAfter(() => onSelectModel(model.id)),
      })),
    ];

    return [...sessionResults, ...workspaceResults, ...settingsResults, ...commandResults];
  }, [
    activeSessionId,
    models,
    onClose,
    onNewSession,
    onOpenSettings,
    onOpenWorkspaceSwitcher,
    onSelectModel,
    onSelectSession,
    onToggleReview,
    onToggleSessions,
    query,
    selectedModelId,
    sessions,
    workspace,
    workspaceNames,
  ]);

  const filteredResults = useMemo(
    () => allResults.filter((result) => includesQuery(result, query)),
    [allResults, query],
  );
  const groupedResults = useMemo(() => groupResults(filteredResults), [filteredResults]);

  useFocusTrap({
    active: open,
    containerRef: dialogRef,
    initialFocusRef: inputRef,
    restoreFocusRef,
    onEscape: onClose,
  });

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
  }, [open]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  if (!open) return null;

  const runActive = () => {
    const result = filteredResults[activeIndex];
    if (!result) return;
    result.action();
  };

  let resultIndex = 0;

  return (
    <>
      <div className="command-palette-scrim" onClick={onClose} aria-hidden="true" />
      <div
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        ref={dialogRef}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setActiveIndex((current) => Math.min(current + 1, Math.max(filteredResults.length - 1, 0)));
            return;
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            setActiveIndex((current) => Math.max(current - 1, 0));
            return;
          }
          if (event.key === "Enter") {
            event.preventDefault();
            runActive();
          }
        }}
      >
        <div className="border-b border-surface-overlay p-3 bg-surface-elevated">
          <input
            ref={inputRef}
            className="h-11 w-full rounded-lg bg-surface-base px-3 font-body text-body text-text-primary outline-none placeholder:text-text-muted border border-surface-overlay"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search sessions, settings, models, commands"
            aria-label="Search commands"
          />
        </div>

        <div className="command-palette-list bg-surface-elevated">
          {groupedResults.length === 0 && (
            <div className="px-3 py-6 text-center text-body-sm text-text-muted">
              No matching commands.
            </div>
          )}

          {groupedResults.map((group) => (
            <section className="mb-2 last:mb-0" key={group.label}>
              <h2 className="m-0 px-3 py-2 font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
                {group.label}
              </h2>
              <div className="grid gap-1">
                {group.results.map((result) => {
                  const index = resultIndex++;
                  const isActive = index === activeIndex;
                  return (
                    <button
                      key={result.id}
                      type="button"
                      className={`command-palette-result ${isActive ? "is-active" : ""}`}
                      onClick={result.action}
                      onMouseEnter={() => setActiveIndex(index)}
                      aria-current={isActive ? "true" : undefined}
                    >
                      <span className="command-palette-icon rounded-lg bg-surface-base" aria-hidden="true">
                        {result.icon}
                      </span>
                      <span className="min-w-0">
                        <span className="block truncate text-body-sm font-medium text-text-primary">
                          {result.label}
                        </span>
                        {result.detail && (
                          <span className="block truncate font-mono text-caption text-text-muted">
                            {result.detail}
                          </span>
                        )}
                      </span>
                      {result.shortcut && (
                        <span className="rounded-lg bg-surface-base px-1.5 py-0.5 font-mono text-caption text-text-muted">
                          {result.shortcut}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            </section>
          ))}
        </div>
      </div>
    </>
  );
}
