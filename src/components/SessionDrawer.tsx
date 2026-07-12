import { useEffect, useRef, useState, type RefObject } from "react";
import {
  Archive,
  CheckSquare,
  Download,
  Lock,
  RotateCcw,
  Square,
  Trash2,
  Upload,
} from "lucide-react";
import type { ArchiveStats, Session } from "../types";
import { useFocusTrap } from "../hooks/useFocusTrap";

interface SessionDrawerProps {
  sessions: Session[];
  archivedSessions?: Session[];
  activeSessionId?: string;
  activeWorkspaceId?: string;
  onSelect: (session: Session) => void;
  onNewSession: () => void;
  onClose: () => void;
  showArchived?: boolean;
  onShowArchivedChange?: (showArchived: boolean) => void;
  onArchiveSession?: (session: Session) => void;
  onRestoreSession?: (session: Session) => void;
  onDeleteArchivedSession?: (session: Session) => void;
  onArchiveSessions?: (sessions: Session[]) => void;
  onRestoreArchivedSessions?: (sessions: Session[]) => void;
  onDeleteArchivedSessions?: (sessions: Session[]) => void;
  onExportArchivedSessions?: (sessions: Session[]) => void;
  onImportArchivedSessions?: (payload: string) => void;
  onDeleteExpiredArchivedSessions?: () => void;
  archiveStats?: ArchiveStats | null;
  archiveActionsDisabled?: boolean;
  open: boolean;
  triggerRef?: RefObject<HTMLElement>;
  trapPaused?: boolean;
  workspaceNames?: Record<string, string>;
}

function setInert(element: HTMLElement, inert: boolean) {
  if (!(element instanceof HTMLElement)) return;
  (element as HTMLElement & { inert?: boolean }).inert = inert;
}

function groupByDate(sessions: Session[]): Record<string, Session[]> {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today.getTime() - 86400000);
  const weekAgo = new Date(today.getTime() - 7 * 86400000);

  const groups: Record<string, Session[]> = {
    Today: [],
    Yesterday: [],
    "This Week": [],
    Older: [],
  };

  for (const session of sessions) {
    const d = new Date(session.timestamp);
    const sessionDate = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    if (sessionDate.getTime() === today.getTime()) {
      groups["Today"].push(session);
    } else if (sessionDate.getTime() === yesterday.getTime()) {
      groups["Yesterday"].push(session);
    } else if (sessionDate.getTime() >= weekAgo.getTime()) {
      groups["This Week"].push(session);
    } else {
      groups["Older"].push(session);
    }
  }

  return groups;
}

export function SessionDrawer({
  sessions,
  archivedSessions = [],
  activeSessionId,
  activeWorkspaceId,
  onSelect,
  onNewSession,
  onClose,
  showArchived = false,
  onShowArchivedChange,
  onArchiveSession,
  onRestoreSession,
  onDeleteArchivedSession,
  onArchiveSessions,
  onRestoreArchivedSessions,
  onDeleteArchivedSessions,
  onExportArchivedSessions,
  onImportArchivedSessions,
  onDeleteExpiredArchivedSessions,
  archiveStats,
  archiveActionsDisabled = false,
  open,
  triggerRef,
  trapPaused = false,
  workspaceNames,
}: SessionDrawerProps) {
  const [search, setSearch] = useState("");
  const [workspaceFilter, setWorkspaceFilter] = useState("all");
  const [dateRangeFilter, setDateRangeFilter] = useState("all");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [importOpen, setImportOpen] = useState(false);
  const [importPayload, setImportPayload] = useState("");
  const drawerRef = useRef<HTMLElement>(null);
  const visibleSessions = showArchived ? archivedSessions : sessions;
  const hasArchiveMode = Boolean(onShowArchivedChange);
  const selectionEnabled = showArchived
    ? Boolean(onRestoreArchivedSessions || onDeleteArchivedSessions || onExportArchivedSessions)
    : Boolean(onArchiveSessions);
  const workspaceOptions = Array.from(
    new Set(visibleSessions.map((session) => session.workspaceId).filter(Boolean) as string[])
  );
  const filteredSessions = visibleSessions.filter((session) => {
    const normalizedSearch = search.toLowerCase();
    const modelLabel = session.variant ? `${session.model} ${session.variant}` : session.model;
    const matchesSearch =
      session.title.toLowerCase().includes(normalizedSearch) ||
      modelLabel.toLowerCase().includes(normalizedSearch);
    const matchesWorkspace = workspaceFilter === "all" || session.workspaceId === workspaceFilter;
    const rangeDays = dateRangeFilter === "all" ? null : Number(dateRangeFilter);
    const matchesDate =
      !showArchived ||
      rangeDays === null ||
      session.timestamp.getTime() >= Date.now() - rangeDays * 86400000;
    return matchesSearch && matchesWorkspace && matchesDate;
  });
  const groups = groupByDate(filteredSessions);
  const groupEntries = Object.entries(groups).filter(([, items]) => items.length > 0);
  const hasCrossWorkspaceSessions =
    new Set(filteredSessions.map((session) => session.workspaceId).filter(Boolean)).size > 1;
  const selectedSessions = filteredSessions.filter((session) => selectedIds.has(session.id));
  const allVisibleSelected =
    filteredSessions.length > 0 && filteredSessions.every((session) => selectedIds.has(session.id));
  const activeWorkspaceSessions = sessions.filter((session) =>
    activeWorkspaceId ? session.workspaceId === activeWorkspaceId : true
  );

  const getWorkspaceLabel = (session: Session) => {
    if (!session.workspaceId) return null;
    if (session.workspaceId === activeWorkspaceId) return null;
    return workspaceNames?.[session.workspaceId] || session.workspaceId;
  };

  useFocusTrap({
    active: open && !trapPaused,
    containerRef: drawerRef,
    onEscape: onClose,
    restoreFocusRef: triggerRef,
    shouldRestoreFocusOnDeactivate: !trapPaused,
  });

  useEffect(() => {
    const drawer = drawerRef.current;
    if (!drawer) return;
    setInert(drawer, !open);
  }, [open]);

  useEffect(() => {
    setSelectedIds(new Set());
  }, [showArchived, workspaceFilter, dateRangeFilter, search]);

  const toggleSelected = (session: Session) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(session.id)) {
        next.delete(session.id);
      } else {
        next.add(session.id);
      }
      return next;
    });
  };

  const toggleSelectVisible = () => {
    setSelectedIds((prev) => {
      if (allVisibleSelected) return new Set();
      const next = new Set(prev);
      for (const session of filteredSessions) {
        next.add(session.id);
      }
      return next;
    });
  };

  return (
    <>
      <div
        className="session-scrim"
        style={{ opacity: open ? 1 : 0, pointerEvents: open ? "auto" : "none" }}
        onClick={onClose}
        aria-hidden="true"
      />
      <aside
        className={`session-drawer ${open ? "is-open" : ""}`}
        ref={drawerRef}
        role="dialog"
        aria-label="Session history"
        aria-modal={open ? "true" : undefined}
        aria-hidden={!open}
        tabIndex={-1}
      >
        <div className="border-b border-surface-overlay shrink-0">
          <div className="flex items-center gap-2 p-3">
            <svg
              className="text-text-muted shrink-0"
              width="14"
              height="14"
              viewBox="0 0 14 14"
              fill="none"
            >
              <circle cx="5.5" cy="5.5" r="4" stroke="currentColor" strokeWidth="1.5" />
              <path
                d="M9 9L12.5 12.5"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
            <input
              className="h-11 flex-1 font-body text-body-sm text-text-primary placeholder:text-text-muted bg-transparent"
              type="text"
              placeholder={showArchived ? "Search archived sessions..." : "Search sessions..."}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          {hasArchiveMode && (
            <div className="session-mode-switch" role="group" aria-label="Session view">
              <button
                type="button"
                className={`session-mode-button ${!showArchived ? "is-active" : ""}`}
                aria-pressed={!showArchived}
                onClick={() => onShowArchivedChange?.(false)}
              >
                Recent
              </button>
              <button
                type="button"
                className={`session-mode-button ${showArchived ? "is-active" : ""}`}
                aria-pressed={showArchived}
                onClick={() => onShowArchivedChange?.(true)}
              >
                Archived
              </button>
            </div>
          )}
          <div className="session-management-bar">
            {selectionEnabled && (
              <button
                type="button"
                className="session-management-button"
                onClick={toggleSelectVisible}
                disabled={filteredSessions.length === 0}
              >
                {allVisibleSelected ? "Clear" : "Select visible"}
              </button>
            )}
            {!showArchived && onArchiveSessions && (
              <>
                <button
                  type="button"
                  className="session-management-button"
                  disabled={archiveActionsDisabled || selectedSessions.length === 0}
                  onClick={() => {
                    onArchiveSessions(selectedSessions);
                    setSelectedIds(new Set());
                  }}
                >
                  Archive selected
                </button>
                <button
                  type="button"
                  className="session-management-button"
                  disabled={archiveActionsDisabled || activeWorkspaceSessions.length === 0}
                  onClick={() => onArchiveSessions(activeWorkspaceSessions)}
                >
                  Archive workspace
                </button>
              </>
            )}
            {showArchived && (
              <>
                <span className="session-management-stat">
                  {archiveStats
                    ? `${archiveStats.archived_count} archived | ${archiveStats.expired_count} expired`
                    : `${archivedSessions.length} archived`}
                </span>
                {archiveStats &&
                  archiveStats.expired_count > 0 &&
                  onDeleteExpiredArchivedSessions && (
                    <button
                      type="button"
                      className="session-management-button is-danger"
                      disabled={archiveActionsDisabled}
                      onClick={onDeleteExpiredArchivedSessions}
                    >
                      Delete expired
                    </button>
                  )}
              </>
            )}
          </div>
          {showArchived && (
            <div className="session-filter-row">
              <select
                className="session-filter-select"
                value={workspaceFilter}
                onChange={(event) => setWorkspaceFilter(event.target.value)}
                aria-label="Workspace filter"
              >
                <option value="all">All workspaces</option>
                {workspaceOptions.map((workspaceId) => (
                  <option key={workspaceId} value={workspaceId}>
                    {workspaceNames?.[workspaceId] || workspaceId}
                  </option>
                ))}
              </select>
              <select
                className="session-filter-select"
                value={dateRangeFilter}
                onChange={(event) => setDateRangeFilter(event.target.value)}
                aria-label="Date filter"
              >
                <option value="all">All dates</option>
                <option value="7">7 days</option>
                <option value="30">30 days</option>
                <option value="90">90 days</option>
                <option value="365">365 days</option>
              </select>
            </div>
          )}
          {showArchived && selectedSessions.length > 0 && (
            <div className="session-bulk-actions">
              {onRestoreArchivedSessions && (
                <button
                  type="button"
                  className="session-icon-button"
                  title="Restore selected"
                  aria-label="Restore selected archived sessions"
                  disabled={archiveActionsDisabled}
                  onClick={() => {
                    onRestoreArchivedSessions(selectedSessions);
                    setSelectedIds(new Set());
                  }}
                >
                  <RotateCcw aria-hidden="true" />
                </button>
              )}
              {onExportArchivedSessions && (
                <button
                  type="button"
                  className="session-icon-button"
                  title="Export selected"
                  aria-label="Export selected archived sessions"
                  disabled={archiveActionsDisabled}
                  onClick={() => onExportArchivedSessions(selectedSessions)}
                >
                  <Download aria-hidden="true" />
                </button>
              )}
              {onDeleteArchivedSessions && (
                <button
                  type="button"
                  className="session-icon-button is-danger"
                  title="Delete selected"
                  aria-label="Delete selected archived sessions"
                  disabled={archiveActionsDisabled}
                  onClick={() => {
                    onDeleteArchivedSessions(selectedSessions);
                    setSelectedIds(new Set());
                  }}
                >
                  <Trash2 aria-hidden="true" />
                </button>
              )}
            </div>
          )}
          {showArchived && onImportArchivedSessions && (
            <div className="session-import-panel">
              {importOpen ? (
                <>
                  <textarea
                    className="session-import-input"
                    value={importPayload}
                    onChange={(event) => setImportPayload(event.target.value)}
                    placeholder="Paste archive JSON"
                    aria-label="Archive import JSON"
                  />
                  <div className="session-import-actions">
                    <button
                      type="button"
                      className="session-management-button"
                      onClick={() => {
                        setImportOpen(false);
                        setImportPayload("");
                      }}
                    >
                      Cancel
                    </button>
                    <button
                      type="button"
                      className="session-management-button"
                      disabled={!importPayload.trim() || archiveActionsDisabled}
                      onClick={() => {
                        onImportArchivedSessions(importPayload);
                        setImportOpen(false);
                        setImportPayload("");
                      }}
                    >
                      Import
                    </button>
                  </div>
                </>
              ) : (
                <button
                  type="button"
                  className="session-management-button"
                  onClick={() => setImportOpen(true)}
                >
                  <Upload aria-hidden="true" />
                  Import archive
                </button>
              )}
            </div>
          )}
        </div>
        <div className="flex-1 overflow-y-auto py-2">
          {groupEntries.length === 0 && (
            <div className="px-3 py-8 text-body-sm text-text-muted">
              {showArchived ? "No archived sessions" : "No sessions"}
            </div>
          )}
          {groupEntries.map(([label, items]) => (
            <div key={label} className="mb-2">
              <div className="text-caption font-medium text-text-muted tracking-[0.02em] uppercase py-2 px-3">
                {label}
              </div>
              {items.map((session) => {
                const isActive = session.id === activeSessionId;
                const title = session.title || "Untitled";
                const isSelected = selectedIds.has(session.id);
                const rowContent = (
                  <>
                    <span className="session-row-dot" aria-hidden="true" />
                    <div className="min-w-0">
                      <div className="text-body-sm text-text-primary overflow-hidden text-ellipsis whitespace-nowrap">
                        {title}
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-caption text-text-muted">
                          {session.variant
                            ? `${session.model} · ${session.variant}`
                            : session.model}
                        </span>
                        {session.mode === "ReadOnly" && (
                          <span className="session-readonly-badge">
                            <Lock aria-hidden="true" />
                            Plan
                          </span>
                        )}
                        {hasCrossWorkspaceSessions &&
                          workspaceNames &&
                          getWorkspaceLabel(session) && (
                            <span className="max-w-40 truncate rounded-full border border-surface-overlay px-2 py-0.5 text-caption text-text-muted">
                              {getWorkspaceLabel(session)}
                            </span>
                          )}
                        <time className="font-mono text-caption text-text-muted">
                          {session.timestamp.toLocaleTimeString([], {
                            hour: "2-digit",
                            minute: "2-digit",
                          })}
                        </time>
                      </div>
                    </div>
                  </>
                );
                return (
                  <div key={session.id} className={`session-row ${isActive ? "is-active" : ""}`}>
                    {selectionEnabled && (
                      <button
                        type="button"
                        className="session-row-select"
                        aria-label={`${isSelected ? "Deselect" : "Select"} ${title}`}
                        aria-pressed={isSelected}
                        onClick={() => toggleSelected(session)}
                      >
                        {isSelected ? (
                          <CheckSquare aria-hidden="true" />
                        ) : (
                          <Square aria-hidden="true" />
                        )}
                      </button>
                    )}
                    {showArchived ? (
                      <div className="session-row-main">{rowContent}</div>
                    ) : (
                      <button
                        type="button"
                        className="session-row-main"
                        onClick={() => onSelect(session)}
                        aria-current={isActive ? "true" : undefined}
                      >
                        {rowContent}
                      </button>
                    )}
                    {showArchived ? (
                      <div className="session-row-actions" aria-label={`Actions for ${title}`}>
                        {(onRestoreSession || onRestoreArchivedSessions) && (
                          <button
                            type="button"
                            className="session-row-action"
                            aria-label={`Restore ${title}`}
                            title="Restore"
                            disabled={archiveActionsDisabled}
                            onClick={() => {
                              if (onRestoreSession) {
                                onRestoreSession(session);
                              } else {
                                onRestoreArchivedSessions?.([session]);
                              }
                            }}
                          >
                            <RotateCcw aria-hidden="true" />
                          </button>
                        )}
                        {onExportArchivedSessions && (
                          <button
                            type="button"
                            className="session-row-action"
                            aria-label={`Export ${title}`}
                            title="Export"
                            disabled={archiveActionsDisabled}
                            onClick={() => onExportArchivedSessions([session])}
                          >
                            <Download aria-hidden="true" />
                          </button>
                        )}
                        {(onDeleteArchivedSession || onDeleteArchivedSessions) && (
                          <button
                            type="button"
                            className="session-row-action is-danger"
                            aria-label={`Delete ${title} permanently`}
                            title="Delete permanently"
                            disabled={archiveActionsDisabled}
                            onClick={() => {
                              if (onDeleteArchivedSession) {
                                onDeleteArchivedSession(session);
                              } else {
                                onDeleteArchivedSessions?.([session]);
                              }
                            }}
                          >
                            <Trash2 aria-hidden="true" />
                          </button>
                        )}
                      </div>
                    ) : (
                      (onArchiveSession || onArchiveSessions) && (
                        <button
                          type="button"
                          className="session-row-action"
                          aria-label={`Archive ${title}`}
                          title="Archive"
                          disabled={archiveActionsDisabled}
                          onClick={() => {
                            if (onArchiveSession) {
                              onArchiveSession(session);
                            } else {
                              onArchiveSessions?.([session]);
                            }
                          }}
                        >
                          <Archive aria-hidden="true" />
                        </button>
                      )
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
        <button
          className="flex min-h-11 items-center justify-center gap-2 w-full p-3 border-t border-surface-overlay text-body-sm text-text-muted transition-colors duration-150 ease-out-quart shrink-0 hover:bg-surface-overlay hover:text-text-secondary"
          onClick={onNewSession}
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path
              d="M7 3V11M3 7H11"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
          New session
        </button>
      </aside>
    </>
  );
}
