import { Check, Plus, Search, X } from "lucide-react";
import { useRef, useState } from "react";
import { useFocusTrap } from "../hooks/useFocusTrap";
import type { Workspace } from "../types";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { WorkspaceDetails } from "./WorkspacePicker";

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
  const searchInputRef = useRef<HTMLInputElement>(null);
  useFocusTrap({
    active: !trapPaused,
    containerRef: dialogRef,
    initialFocusRef: searchInputRef,
    onEscape: onClose,
    shouldRestoreFocusOnDeactivate: !trapPaused,
  });
  const filtered = workspaces.filter((w) =>
    `${w.name} ${w.path}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase())
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
      >
        <div className="workspace-switcher-heading">
          <h2>Workspaces</h2>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onClose}
            aria-label="Close workspace picker"
          >
            <X size={16} aria-hidden="true" />
          </Button>
        </div>
        <div className="combobox-search">
          <Search size={15} aria-hidden="true" />
          <Input
            ref={searchInputRef}
            aria-label="Search workspaces"
            placeholder="Search by name or folder…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className="workspace-switcher-list">
          {loading && workspaces.length === 0 && (
            <p className="workspace-switcher-empty">Loading workspaces…</p>
          )}
          {!loading && filtered.length === 0 && (
            <p className="workspace-switcher-empty">
              {workspaces.length === 0
                ? "Add a project folder to get started."
                : "No workspaces found."}
            </p>
          )}
          {filtered.map((ws) => (
            <div key={ws.id} className="workspace-row">
              <Button
                variant="ghost"
                className="workspace-switcher-option"
                aria-current={ws.id === activeWorkspaceId ? "true" : undefined}
                onClick={() => {
                  onSelect(ws);
                  onClose();
                }}
              >
                <WorkspaceDetails workspace={ws} />
                {ws.sessionCount > 0 && (
                  <span className="workspace-session-count">{ws.sessionCount}</span>
                )}
                {ws.id === activeWorkspaceId && (
                  <Check size={15} aria-hidden="true" className="workspace-current-check" />
                )}
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => onRemove(ws.id)}
                aria-label={`Remove workspace ${ws.name}`}
                title={`Remove workspace ${ws.name}`}
              >
                <X size={14} aria-hidden="true" />
              </Button>
            </div>
          ))}
        </div>
        <div className="workspace-picker-footer">
          <Button variant="ghost" onClick={onAdd}>
            <Plus size={15} aria-hidden="true" />
            Add workspace
          </Button>
        </div>
      </div>
    </>
  );
}
