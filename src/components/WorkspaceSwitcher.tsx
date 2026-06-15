import { useRef, useState } from "react";
import type { Workspace } from "../types";
import { useFocusTrap } from "../hooks/useFocusTrap";

interface WorkspaceSwitcherProps {
  workspaces: Workspace[];
  activeWorkspaceId: string;
  onSelect: (workspace: Workspace) => void;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onClose: () => void;
  loading?: boolean;
  trapPaused?: boolean;
}

export function WorkspaceSwitcher({
  workspaces,
  activeWorkspaceId,
  onSelect,
  onAdd,
  onRemove,
  onClose,
  loading,
  trapPaused = false,
}: WorkspaceSwitcherProps) {
  const [search, setSearch] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const open = true;

  useFocusTrap({
    active: open && !trapPaused,
    containerRef: dialogRef,
    onEscape: onClose,
    restoreFocusOnDeactivate: !trapPaused,
  });

  const filtered = workspaces.filter((w) =>
    w.name.toLowerCase().includes(search.toLowerCase()) ||
    w.path.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <>
      <div
        className="fixed inset-0 bg-scrim z-[var(--z-scrim)]"
        onClick={onClose}
        aria-hidden="true"
      />
      <div
        className="workspace-switcher-dialog"
        role="dialog"
        aria-label="Switch workspace"
        aria-modal="true"
        ref={dialogRef}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 p-3 border-b border-surface-overlay">
          <svg
            className="text-text-muted shrink-0"
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
          >
            <circle cx="5.5" cy="5.5" r="4" stroke="currentColor" strokeWidth="1.5" />
            <path d="M9 9L12.5 12.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          <input
            className="h-11 flex-1 font-body text-body-sm text-text-primary placeholder:text-text-muted bg-transparent"
            type="text"
            placeholder="Search workspaces..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            autoFocus
          />
        </div>
        <div className="overflow-y-auto flex-1">
          {loading && workspaces.length === 0 && (
            <div className="p-4 text-center text-text-muted text-body-sm">Loading workspaces...</div>
          )}
          {!loading && workspaces.length === 0 && (
            <div className="p-4 text-center text-text-muted text-body-sm">No workspaces yet. Add one below.</div>
          )}
          {filtered.map((ws) => {
            const isActive = ws.id === activeWorkspaceId;
            return (
              <div
                key={ws.id}
                className={`workspace-row group ${isActive ? "is-active" : ""}`}
              >
                <span
                  className={`workspace-row-dot ${isActive ? "" : "opacity-0"}`}
                  aria-hidden="true"
                />
                <button
                  className="flex min-h-11 items-center gap-3 flex-1 py-3 px-4 text-left bg-transparent border-none cursor-pointer text-inherit font-inherit min-w-0"
                  onClick={() => {
                    onSelect(ws);
                    onClose();
                  }}
                >
                  <div className="text-body font-medium text-text-primary min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">{ws.name}</div>
                  <div className="font-mono text-caption text-text-muted min-w-0 overflow-hidden text-ellipsis whitespace-nowrap flex-1">{ws.path}</div>
                  {ws.sessionCount > 0 && (
                    <span className="font-mono text-caption text-text-muted bg-surface-overlay px-1 rounded-sm shrink-0">
                      {ws.sessionCount}
                    </span>
                  )}
                </button>
                <button
                  className="hit-target flex items-center justify-center w-6 h-6 mr-2 border-none bg-transparent text-text-muted cursor-pointer rounded-sm text-sm leading-none shrink-0 opacity-0 transition-opacity duration-150 ease-out-quart group-hover:opacity-100 hover:text-text-primary focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-accent-action focus-visible:outline-offset-2"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRemove(ws.id);
                  }}
                  aria-label="Remove workspace"
                  title="Remove workspace"
                >
                  ×
                </button>
              </div>
            );
          })}
        </div>
        <button
          className="flex min-h-11 items-center justify-center gap-2 w-full p-3 border-t border-surface-overlay text-body-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
          onClick={onAdd}
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path d="M7 3V11M3 7H11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          Add workspace
        </button>
      </div>
    </>
  );
}
