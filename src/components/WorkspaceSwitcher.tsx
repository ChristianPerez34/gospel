import { useState, useEffect, useCallback, useRef } from "react";
import type { Workspace } from "../types";
import "./WorkspaceSwitcher.css";

interface WorkspaceSwitcherProps {
  workspaces: Workspace[];
  activeWorkspaceId: string;
  onSelect: (workspace: Workspace) => void;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onClose: () => void;
  loading?: boolean;
}

export function WorkspaceSwitcher({
  workspaces,
  activeWorkspaceId,
  onSelect,
  onAdd,
  onRemove,
  onClose,
  loading,
}: WorkspaceSwitcherProps) {
  const [search, setSearch] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
    }
    if (e.key === "Tab" && dialogRef.current) {
      const focusableElements = dialogRef.current.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      const firstElement = focusableElements[0] as HTMLElement;
      const lastElement = focusableElements[focusableElements.length - 1] as HTMLElement;

      if (e.shiftKey) {
        if (document.activeElement === firstElement) {
          e.preventDefault();
          lastElement.focus();
        }
      } else {
        if (document.activeElement === lastElement) {
          e.preventDefault();
          firstElement.focus();
        }
      }
    }
  }, [onClose]);

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  const filtered = workspaces.filter((w) =>
    w.name.toLowerCase().includes(search.toLowerCase()) ||
    w.path.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <>
      <div
        className="workspace-switcher__scrim"
        onClick={onClose}
        aria-hidden="true"
      />
      <div
        className="workspace-switcher"
        role="dialog"
        aria-label="Switch workspace"
        aria-modal="true"
        ref={dialogRef}
        onClick={(e) => e.stopPropagation()}
      >
      <div className="workspace-switcher__search">
        <svg
          className="workspace-switcher__search-icon"
          width="14"
          height="14"
          viewBox="0 0 14 14"
          fill="none"
        >
          <circle
            cx="5.5"
            cy="5.5"
            r="4"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="M9 9L12.5 12.5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
        </svg>
        <input
          className="workspace-switcher__search-input"
          type="text"
          placeholder="Search workspaces..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          autoFocus
        />
      </div>
      <div className="workspace-switcher__list">
        {loading && workspaces.length === 0 && (
          <div className="workspace-switcher__empty">Loading workspaces...</div>
        )}
        {!loading && workspaces.length === 0 && (
          <div className="workspace-switcher__empty">No workspaces yet. Add one below.</div>
        )}
        {filtered.map((ws) => (
          <div key={ws.id} className={`workspace-switcher__item${
            ws.id === activeWorkspaceId
              ? " workspace-switcher__item--active"
              : ""
          }`}>
            <button
              className="workspace-switcher__item-content"
              onClick={() => {
                onSelect(ws);
                onClose();
              }}
            >
              <div className="workspace-switcher__item-name">{ws.name}</div>
              <div className="workspace-switcher__item-path">{ws.path}</div>
              {ws.sessionCount > 0 && (
                <span className="workspace-switcher__item-count">
                  {ws.sessionCount}
                </span>
              )}
            </button>
            <button
              className="workspace-switcher__item-remove"
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
        ))}
      </div>
      <button className="workspace-switcher__add" onClick={onAdd}>
        <svg
          width="14"
          height="14"
          viewBox="0 0 14 14"
          fill="none"
        >
          <path
            d="M7 3V11M3 7H11"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
        </svg>
        Add workspace
      </button>
      </div>
    </>
  );
}