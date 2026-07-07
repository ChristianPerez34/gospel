import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TopBar } from "./TopBar";
import { ChatView } from "./ChatView";
import { InputBar } from "./InputBar";
import { SessionDrawer } from "./SessionDrawer";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { SettingsModal } from "./SettingsModal";
import { ReviewPanel } from "./ReviewPanel";
import { CommandPalette } from "./CommandPalette";
import { ToastContainer, useToasts } from "./Toast";
import { useWorkspaces } from "../hooks/useWorkspaces";
import { useModelAvailability } from "../hooks/useModelAvailability";
import { useSessionManager } from "../hooks/useSessionManager";
import { useThemePreference } from "../hooks/useThemePreference";
import {
  modelOptionId,
  normalizeSessionMode,
  type ArchivePolicy,
  type ArchiveStats,
  type Session,
} from "../types";
import { noModelCopy } from "../modelAvailabilityCopy";

type SettingsTab = "general" | "models" | "data";
type TrappedSurface = "sessions" | "review" | null;

interface BackendSessionRecord {
  id: string;
  title: string;
  provider: string;
  model: string;
  variant?: string | null;
  status: string;
  mode?: string | null;
  workspace_id: string | null;
  updated_at: string;
}

interface BackendArchivedSessionRecord extends BackendSessionRecord {
  archived_at: string;
}

function sortSessionsByTimestamp(items: Session[]): Session[] {
  return [...items].sort(
    (left, right) => right.timestamp.getTime() - left.timestamp.getTime(),
  );
}

function mergeLoadedSessions(loadedSessions: Session[], existingSessions: Session[]): Session[] {
  const byId = new Map(loadedSessions.map((item) => [item.id, item]));

  for (const existing of existingSessions) {
    const loaded = byId.get(existing.id);
    if (loaded && existing.messages.length > 0) {
      byId.set(existing.id, {
        ...loaded,
        messages: existing.messages,
        status: existing.status,
        timestamp:
          existing.timestamp.getTime() > loaded.timestamp.getTime()
            ? existing.timestamp
            : loaded.timestamp,
      });
    } else if (!loaded && (!existing.backendCreated || existing.messages.length > 0)) {
      byId.set(existing.id, existing);
    }
  }

  return sortSessionsByTimestamp(Array.from(byId.values()));
}

function groupSessionsByWorkspace(items: Session[]): Session[][] {
  const groups = new Map<string, Session[]>();
  for (const item of items) {
    const key = item.workspaceId ?? "";
    groups.set(key, [...(groups.get(key) ?? []), item]);
  }
  return Array.from(groups.values());
}

export function AppShell() {
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [workspaceSwitcherOpen, setWorkspaceSwitcherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsInitialTab, setSettingsInitialTab] = useState<SettingsTab>("models");
  const [reviewOpen, setReviewOpen] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const sessionToggleRef = useRef<HTMLButtonElement>(null);
  const reviewToggleRef = useRef<HTMLButtonElement>(null);
  const commandPaletteRestoreRef = useRef<HTMLElement | null>(null);
  const commandPaletteOpenRef = useRef(false);
  const chatColumnRef = useRef<HTMLDivElement>(null);
  const { themePreference, resolvedTheme, setThemePreference } = useThemePreference();

  const trappedSurface: TrappedSurface = sessionDrawerOpen
    ? "sessions"
    : reviewOpen
      ? "review"
      : null;

  const openSettings = useCallback((tab: SettingsTab = "models") => {
    setSettingsInitialTab(tab);
    setSettingsOpen(true);
  }, []);

  const { workspaces, activeWorkspace, addWorkspace, removeWorkspace, switchWorkspace, loading: workspacesLoading } = useWorkspaces();
  const { toasts, dismissToast, showError, showSuccess } = useToasts();
  const {
    models,
    providers,
    setProviders,
    selectedModel,
    setSelectedModel,
    availabilitySnapshot,
    isRefreshingModels,
    refreshModelAvailability,
  } = useModelAvailability({
    onError: showError,
    onSuccess: showSuccess,
  });

  const [sessions, setSessions] = useState<Session[]>([]);
  const [archivedSessions, setArchivedSessions] = useState<Session[]>([]);
  const [showArchivedSessions, setShowArchivedSessions] = useState(false);
  const [archivePolicy, setArchivePolicy] = useState<ArchivePolicy | null>(null);
  const [workspaceArchivePolicy, setWorkspaceArchivePolicy] = useState<ArchivePolicy | null>(null);
  const [archiveStats, setArchiveStats] = useState<ArchiveStats | null>(null);
  const [archivePolicySaving, setArchivePolicySaving] = useState(false);

  const session = useSessionManager({
    models,
    selectedModel,
    sessions,
    onSessionsChange: setSessions,
    activeWorkspaceId: activeWorkspace?.id,
    onSwitchWorkspace: switchWorkspace,
    onError: showError,
    onSuccess: showSuccess,
    onOpenSettings: openSettings,
  });

  const workspaceNames = useMemo<Record<string, string>>(() => {
    const names: Record<string, string> = {};
    for (const item of workspaces) {
      names[item.id] = item.name;
    }
    return names;
  }, [workspaces]);

  const mapBackendSessions = useCallback((backendSessions: BackendSessionRecord[]): Session[] => {
    return backendSessions.map((session) => ({
      id: session.id,
      title: session.title,
      provider: session.provider,
      model: session.model,
      variant: session.variant ?? null,
      mode: normalizeSessionMode(session.mode),
      timestamp: new Date(session.updated_at),
      messages: [],
      status: (session.status === "active" ? "idle" : "error") as Session["status"],
      backendCreated: true,
      workspaceId: session.workspace_id ?? undefined,
    }));
  }, []);

  const mapBackendArchivedSessions = useCallback((backendSessions: BackendArchivedSessionRecord[]): Session[] => {
    return backendSessions.map((session) => ({
      id: session.id,
      title: session.title,
      provider: session.provider,
      model: session.model,
      variant: session.variant ?? null,
      mode: normalizeSessionMode(session.mode),
      timestamp: new Date(session.archived_at),
      messages: [],
      status: "archived" as const,
      backendCreated: true,
      workspaceId: session.workspace_id ?? undefined,
      archivedAt: new Date(session.archived_at),
    }));
  }, []);

  const loadArchiveMeta = useCallback(async (workspaceId?: string) => {
    const [globalPolicy, scopedPolicy, stats] = await Promise.all([
      invoke<ArchivePolicy>("get_archive_policy", { workspaceId: null }),
      workspaceId
        ? invoke<ArchivePolicy>("get_archive_policy", { workspaceId })
        : Promise.resolve(null),
      invoke<ArchiveStats>("get_archive_stats", { workspaceId: workspaceId ?? null }),
    ]);
    setArchivePolicy(globalPolicy);
    setWorkspaceArchivePolicy(scopedPolicy);
    setArchiveStats(stats);
  }, []);

  useEffect(() => {
    let cancelled = false;

    const loadWorkspaceSessions = async () => {
      const ids = workspaces.map((item) => item.id);
      if (ids.length === 0) {
        if (!cancelled) {
          setSessions((prev) => prev.filter((item) => !item.backendCreated));
          setArchivedSessions([]);
          await loadArchiveMeta();
        }
        return;
      }

      const [workspaceSessions, workspaceArchivedSessions] = await Promise.all([
        Promise.all(
          ids.map((id) => invoke<BackendSessionRecord[]>("list_sessions", { workspaceId: id })),
        ),
        Promise.all(
          ids.map((id) =>
            invoke<BackendArchivedSessionRecord[]>("list_archived_sessions", { workspaceId: id }),
          ),
        ),
      ]);

      if (cancelled) return;

      const flattened = workspaceSessions.flatMap((backendSessions) => mapBackendSessions(backendSessions));
      const flattenedArchived = workspaceArchivedSessions.flatMap((backendSessions) =>
        mapBackendArchivedSessions(backendSessions),
      );
      setSessions((prev) => {
        return mergeLoadedSessions(flattened, prev);
      });
      setArchivedSessions(sortSessionsByTimestamp(flattenedArchived));
      if (activeWorkspace?.id) {
        await loadArchiveMeta(activeWorkspace.id);
      } else {
        await loadArchiveMeta();
      }
    };

    void loadWorkspaceSessions();

    return () => {
      cancelled = true;
    };
  }, [activeWorkspace?.id, loadArchiveMeta, mapBackendArchivedSessions, mapBackendSessions, workspaces]);

  const reloadArchiveData = useCallback(async () => {
    const ids = workspaces.map((item) => item.id);
    if (ids.length === 0) {
      setArchivedSessions([]);
      await loadArchiveMeta();
      return;
    }

    const [workspaceSessions, workspaceArchivedSessions] = await Promise.all([
      Promise.all(
        ids.map((id) => invoke<BackendSessionRecord[]>("list_sessions", { workspaceId: id })),
      ),
      Promise.all(
        ids.map((id) =>
          invoke<BackendArchivedSessionRecord[]>("list_archived_sessions", { workspaceId: id }),
        ),
      ),
    ]);
    setSessions((prev) =>
      mergeLoadedSessions(workspaceSessions.flatMap(mapBackendSessions), prev),
    );
    setArchivedSessions(sortSessionsByTimestamp(
      workspaceArchivedSessions.flatMap(mapBackendArchivedSessions),
    ));
    await loadArchiveMeta(activeWorkspace?.id);
  }, [
    activeWorkspace?.id,
    loadArchiveMeta,
    mapBackendArchivedSessions,
    mapBackendSessions,
    workspaces,
  ]);

  const allSessions = useMemo(() => {
    return sortSessionsByTimestamp(session.sessions);
  }, [session.sessions]);

  const ensureSessionWorkspaceActive = useCallback(
    async (target: Session) => {
      if (!target.workspaceId || target.workspaceId === activeWorkspace?.id) return true;
      const switched = await switchWorkspace(target.workspaceId);
      if (!switched) {
        showError("Unable to switch workspace for this session.");
      }
      return switched;
    },
    [activeWorkspace?.id, showError, switchWorkspace],
  );

  const handleArchiveSessions = useCallback(
    async (targets: Session[]) => {
      if (session.isStreaming || targets.length === 0) return;

      try {
        for (const group of groupSessionsByWorkspace(targets)) {
          const first = group[0];
          if (!first || !(await ensureSessionWorkspaceActive(first))) return;
          await invoke<BackendArchivedSessionRecord[]>("archive_sessions", {
            sessionIds: group.map((item) => item.id),
          });
        }
        if (targets.some((target) => target.id === session.activeSessionId)) {
          session.handleNewSession();
        }
        await reloadArchiveData();
        showSuccess(targets.length === 1 ? "Session archived." : `${targets.length} sessions archived.`);
      } catch (e) {
        showError(`Failed to archive session: ${e}`);
      }
    },
    [
      ensureSessionWorkspaceActive,
      reloadArchiveData,
      session,
      showError,
      showSuccess,
    ],
  );

  const handleRestoreArchivedSessions = useCallback(
    async (targets: Session[]) => {
      if (session.isStreaming || targets.length === 0) return;

      try {
        for (const group of groupSessionsByWorkspace(targets)) {
          const first = group[0];
          if (!first || !(await ensureSessionWorkspaceActive(first))) return;
          await invoke<BackendSessionRecord[]>("restore_archived_sessions", {
            sessionIds: group.map((item) => item.id),
          });
        }
        await reloadArchiveData();
        showSuccess(targets.length === 1 ? "Session restored." : `${targets.length} sessions restored.`);
      } catch (e) {
        showError(`Failed to restore session: ${e}`);
      }
    },
    [ensureSessionWorkspaceActive, reloadArchiveData, session.isStreaming, showError, showSuccess],
  );

  const handleDeleteArchivedSessions = useCallback(
    async (targets: Session[]) => {
      if (session.isStreaming || targets.length === 0) return;
      const label = targets.length === 1 ? `"${targets[0]?.title || "Untitled"}"` : `${targets.length} sessions`;
      if (!window.confirm(`Delete archived ${label} permanently?`)) return;

      try {
        for (const group of groupSessionsByWorkspace(targets)) {
          const first = group[0];
          if (!first || !(await ensureSessionWorkspaceActive(first))) return;
          await invoke<number>("delete_archived_sessions", {
            sessionIds: group.map((item) => item.id),
          });
        }
        await reloadArchiveData();
        showSuccess(targets.length === 1 ? "Archived session deleted." : `${targets.length} archived sessions deleted.`);
      } catch (e) {
        showError(`Failed to delete archived session: ${e}`);
      }
    },
    [ensureSessionWorkspaceActive, reloadArchiveData, session.isStreaming, showError, showSuccess],
  );

  const handleExportArchivedSessions = useCallback(
    async (targets: Session[]) => {
      if (targets.length === 0) return;

      try {
        const exportedPayloads: string[] = [];
        for (const group of groupSessionsByWorkspace(targets)) {
          const first = group[0];
          if (!first || !(await ensureSessionWorkspaceActive(first))) return;
          exportedPayloads.push(
            await invoke<string>("export_archived_sessions", {
              sessionIds: group.map((item) => item.id),
            }),
          );
        }
        const clipboardPayload =
          exportedPayloads.length === 1
            ? exportedPayloads[0]
            : JSON.stringify(
                {
                  version: 1,
                  exported_at: new Date().toISOString(),
                  sessions: exportedPayloads.flatMap((payload) => JSON.parse(payload).sessions ?? []),
                },
                null,
                2,
              );
        await navigator.clipboard.writeText(clipboardPayload);
        showSuccess(targets.length === 1 ? "Archive export copied." : `${targets.length} archive exports copied.`);
      } catch (e) {
        showError(`Failed to export archive: ${e}`);
      }
    },
    [ensureSessionWorkspaceActive, showError, showSuccess],
  );

  const handleImportArchivedSessions = useCallback(
    async (payload: string) => {
      if (!payload.trim()) return;

      try {
        await invoke<BackendArchivedSessionRecord[]>("import_archived_sessions", {
          payload,
          workspaceId: activeWorkspace?.id ?? null,
        });
        await reloadArchiveData();
        showSuccess("Archive import complete.");
      } catch (e) {
        showError(`Failed to import archive: ${e}`);
      }
    },
    [activeWorkspace?.id, reloadArchiveData, showError, showSuccess],
  );

  const handleDeleteExpiredArchivedSessions = useCallback(async () => {
    if (!archiveStats || archiveStats.expired_count === 0) return;
    if (!window.confirm(`Delete ${archiveStats.expired_count} expired archived sessions permanently?`)) return;

    try {
      await invoke<number>("delete_expired_archived_sessions", {
        workspaceId: activeWorkspace?.id ?? null,
      });
      await reloadArchiveData();
      showSuccess("Expired archived sessions deleted.");
    } catch (e) {
      showError(`Failed to delete expired archives: ${e}`);
    }
  }, [activeWorkspace?.id, archiveStats, reloadArchiveData, showError, showSuccess]);

  const handleArchivePolicyChange = useCallback(
    async (workspaceId: string | null, retentionDays: number, autoArchiveHours: number) => {
      setArchivePolicySaving(true);
      try {
        await invoke<ArchivePolicy>("set_archive_policy", {
          workspaceId,
          retentionDays,
          autoArchiveHours,
        });
        await reloadArchiveData();
        showSuccess("Archive policy updated.");
      } catch (e) {
        showError(`Failed to update archive policy: ${e}`);
      } finally {
        setArchivePolicySaving(false);
      }
    },
    [reloadArchiveData, showError, showSuccess],
  );

  const handleClearWorkspaceArchivePolicy = useCallback(async () => {
    if (!activeWorkspace?.id) return;
    setArchivePolicySaving(true);
    try {
      await invoke<ArchivePolicy>("clear_workspace_archive_policy", {
        workspaceId: activeWorkspace.id,
      });
      await reloadArchiveData();
      showSuccess("Workspace archive policy cleared.");
    } catch (e) {
      showError(`Failed to clear workspace policy: ${e}`);
    } finally {
      setArchivePolicySaving(false);
    }
  }, [activeWorkspace?.id, reloadArchiveData, showError, showSuccess]);

  const handleRunArchiveMaintenance = useCallback(async () => {
    setArchivePolicySaving(true);
    try {
      await invoke("run_archive_maintenance", {
        workspaceId: activeWorkspace?.id ?? null,
      });
      await reloadArchiveData();
      showSuccess("Archive cleanup complete.");
    } catch (e) {
      showError(`Archive cleanup failed: ${e}`);
    } finally {
      setArchivePolicySaving(false);
    }
  }, [activeWorkspace?.id, reloadArchiveData, showError, showSuccess]);

  const activeSession = session.sessions.find((s) => s.id === session.activeSessionId);
  const activeSessionId = activeSession?.id;
  const activeSessionProvider = activeSession?.provider;
  const activeSessionModel = activeSession?.model;
  const activeSessionVariant = activeSession?.variant ?? null;
  const sessionTitle = activeSession?.title || "New session";
  const selectedModelId = selectedModel ? modelOptionId(selectedModel.provider, selectedModel.model) : "";
  const currentModelName = models.find(
    (model) =>
      selectedModel &&
      model.model === selectedModel.model &&
      model.provider.toLowerCase() === selectedModel.provider.toLowerCase(),
  )?.name || selectedModel?.model || "No model";
  const noModels = noModelCopy(availabilitySnapshot);
  const surfaceTrapOpen = trappedSurface !== null;
  const modalSurfaceOpen = commandPaletteOpen || settingsOpen || workspaceSwitcherOpen;

  useEffect(() => {
    if (!activeSessionId || !activeSessionProvider || !activeSessionModel) return;
    setSelectedModel({
      provider: activeSessionProvider,
      model: activeSessionModel,
      variant: activeSessionVariant,
    });
  }, [activeSessionId, activeSessionProvider, activeSessionModel, activeSessionVariant, setSelectedModel]);

  const applyModelSelection = useCallback((modelId: string) => {
    const match = models.find((m) => m.id === modelId);
    if (!match) return;
    const next = {
      provider: match.provider,
      model: match.model,
      variant: null,
    };
    setSelectedModel(next);

    if (!activeSession || session.isStreaming) return;

    const previous = {
      provider: activeSession.provider,
      model: activeSession.model,
      variant: activeSession.variant ?? null,
    };
    setSessions((prev) =>
      prev.map((item) =>
        item.id === activeSession.id
          ? { ...item, provider: next.provider, model: next.model, variant: next.variant }
          : item,
      ),
    );

    if (!activeSession.backendCreated) return;

    void invoke("update_session_model_selection", {
      sessionId: activeSession.id,
      provider: next.provider,
      model: next.model,
      variant: next.variant,
    }).catch((error) => {
      setSessions((prev) =>
        prev.map((item) =>
          item.id === activeSession.id
            ? { ...item, provider: previous.provider, model: previous.model, variant: previous.variant }
            : item,
        ),
      );
      setSelectedModel(previous);
      showError(`Failed to update session model: ${error}`);
    });
  }, [activeSession, models, session.isStreaming, setSelectedModel, showError]);

  const applyVariantSelection = useCallback((variant: string | null) => {
    if (!selectedModel) return;
    const next = { ...selectedModel, variant };
    const previousSelection = selectedModel;
    setSelectedModel(next);

    if (!activeSession || session.isStreaming) return;

    const previous = {
      provider: activeSession.provider,
      model: activeSession.model,
      variant: activeSession.variant ?? null,
    };
    setSessions((prev) =>
      prev.map((item) =>
        item.id === activeSession.id
          ? { ...item, variant }
          : item,
      ),
    );

    if (!activeSession.backendCreated) return;

    void invoke("update_session_model_selection", {
      sessionId: activeSession.id,
      provider: selectedModel.provider,
      model: selectedModel.model,
      variant,
    }).catch((error) => {
      setSessions((prev) =>
        prev.map((item) =>
          item.id === activeSession.id
            ? { ...item, provider: previous.provider, model: previous.model, variant: previous.variant }
            : item,
        ),
      );
      setSelectedModel(previousSelection);
      showError(`Failed to update session model: ${error}`);
    });
  }, [activeSession, selectedModel, session.isStreaming, setSelectedModel, showError]);

  const closeSessionDrawer = useCallback(() => {
    setSessionDrawerOpen(false);
  }, []);

  const closeReviewPanel = useCallback(() => {
    setReviewOpen(false);
  }, []);

  const toggleSessionDrawer = useCallback(() => {
    setSessionDrawerOpen((open) => !open);
  }, []);

  const toggleReviewPanel = useCallback(() => {
    setReviewOpen((open) => !open);
  }, []);

  useEffect(() => {
    const target = chatColumnRef.current as (HTMLDivElement & { inert?: boolean }) | null;
    if (!target) return;
    target.inert = surfaceTrapOpen;
    return () => {
      target.inert = false;
    };
  }, [surfaceTrapOpen]);

  useEffect(() => {
    document.documentElement.dataset.theme = resolvedTheme;
    document.documentElement.dataset.themePreference = themePreference;
  }, [resolvedTheme, themePreference]);

  useEffect(() => {
    commandPaletteOpenRef.current = commandPaletteOpen;
  }, [commandPaletteOpen]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== "k" || (!event.metaKey && !event.ctrlKey)) return;

      event.preventDefault();
      if (commandPaletteOpenRef.current) return;
      commandPaletteRestoreRef.current =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      setCommandPaletteOpen(true);
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  return (
    <div className="app-shell" data-theme={resolvedTheme} data-theme-preference={themePreference}>
      <TopBar
        workspace={activeWorkspace ?? { id: "", name: "No workspace", path: "", sessionCount: 0 }}
        sessionTitle={sessionTitle}
        sessionMode={session.activeSessionMode}
        onSessionModeChange={session.handleSessionModeChange}
        model={currentModelName}
        status={session.status}
        onWorkspaceSwitch={() => setWorkspaceSwitcherOpen(true)}
        onToggleSessions={toggleSessionDrawer}
        onOpenReview={toggleReviewPanel}
        onOpenSettings={openSettings}
        sessionsOpen={sessionDrawerOpen}
        reviewOpen={reviewOpen}
        sessionToggleRef={sessionToggleRef}
        reviewToggleRef={reviewToggleRef}
      />
      <div className="app-layout" data-session-drawer-open={sessionDrawerOpen ? "true" : "false"}>
        <div
          className="app-workspace"
          data-review-open={reviewOpen ? "true" : "false"}
        >
          <div
            className="chat-column"
            ref={chatColumnRef}
            aria-hidden={surfaceTrapOpen ? "true" : undefined}
          >
            <ChatView
              messages={session.messages}
              workspacePath={activeWorkspace?.path ?? ""}
              isThinking={session.isThinking}
              currentTurn={session.currentTurn}
            />
            <InputBar
              models={models}
              selectedModel={selectedModelId}
              selectedVariant={selectedModel?.variant ?? null}
              onModelChange={applyModelSelection}
              onVariantChange={applyVariantSelection}
              onSend={session.handleSend}
              disabled={session.isStreaming || models.length === 0}
              unavailableMessage={models.length === 0 ? noModels.title : "Connecting..."}
              unavailableDetail={noModels.detail}
              unavailableActionLabel={noModels.actionLabel}
              onUnavailableAction={() => openSettings("models")}
              workspacePath={activeWorkspace?.path}
            />
          </div>
          <ReviewPanel
            open={reviewOpen}
            provider={selectedModel?.provider}
            model={selectedModel?.model}
            workspacePath={activeWorkspace?.path}
            canSendTurn={!session.isStreaming}
            onClose={closeReviewPanel}
            onError={showError}
            onSuccess={showSuccess}
            onFixFinding={(prompt) => session.handleSend(prompt)}
            triggerRef={reviewToggleRef}
            trapPaused={modalSurfaceOpen || trappedSurface !== "review"}
          />
        </div>
      </div>
        <SessionDrawer
          sessions={allSessions}
          archivedSessions={archivedSessions}
          activeSessionId={session.activeSessionId ?? undefined}
          workspaceNames={workspaceNames}
          activeWorkspaceId={activeWorkspace?.id}
          showArchived={showArchivedSessions}
          onShowArchivedChange={setShowArchivedSessions}
          archiveStats={archiveStats}
          onSelect={(s) => {
            if (session.isStreaming) return;
            void session.handleSessionSelect(s);
            closeSessionDrawer();
          }}
          onArchiveSessions={(items) => {
            void handleArchiveSessions(items);
          }}
          onRestoreArchivedSessions={(items) => {
            void handleRestoreArchivedSessions(items);
          }}
          onDeleteArchivedSessions={(items) => {
            void handleDeleteArchivedSessions(items);
          }}
          onExportArchivedSessions={(items) => {
            void handleExportArchivedSessions(items);
          }}
          onImportArchivedSessions={(payload) => {
            void handleImportArchivedSessions(payload);
          }}
          onDeleteExpiredArchivedSessions={() => {
            void handleDeleteExpiredArchivedSessions();
          }}
          archiveActionsDisabled={session.isStreaming}
          onNewSession={() => {
            if (session.isStreaming) return;
            session.handleNewSession();
            closeSessionDrawer();
          }}
          onClose={closeSessionDrawer}
          open={sessionDrawerOpen}
          triggerRef={sessionToggleRef}
          trapPaused={modalSurfaceOpen || trappedSurface !== "sessions"}
        />
      {workspaceSwitcherOpen && (
        <WorkspaceSwitcher
          workspaces={workspaces}
          activeWorkspaceId={activeWorkspace?.id ?? ""}
          onSelect={(ws) => { void switchWorkspace(ws.id); }}
          onAdd={() => {
            void (async () => {
              try {
                const path = await invoke<string | null>("pick_workspace_directory");
                if (path) {
                  const result = await addWorkspace(path);
                  if (!result) {
                    showError("Failed to add workspace. It may already exist or the path is invalid.");
                  }
                }
              } catch (e) {
                showError(`Failed to pick workspace directory: ${e}`);
              }
            })();
          }}
          onRemove={(id) => { void removeWorkspace(id); }}
          onClose={() => setWorkspaceSwitcherOpen(false)}
          loading={workspacesLoading}
          trapPaused={commandPaletteOpen || settingsOpen}
        />
      )}
      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        providers={providers}
        onProvidersChange={setProviders}
        onRefreshAvailability={refreshModelAvailability}
        isRefreshingModels={isRefreshingModels}
        initialTab={settingsInitialTab}
        themePreference={themePreference}
        onThemePreferenceChange={setThemePreference}
        archivePolicy={archivePolicy}
        workspaceArchivePolicy={workspaceArchivePolicy}
        archiveStats={archiveStats}
        activeWorkspaceName={activeWorkspace?.name}
        onArchivePolicyChange={handleArchivePolicyChange}
        onClearWorkspaceArchivePolicy={handleClearWorkspaceArchivePolicy}
        onRunArchiveMaintenance={handleRunArchiveMaintenance}
        archivePolicySaving={archivePolicySaving}
      />
      <CommandPalette
        open={commandPaletteOpen}
        sessions={allSessions}
        activeSessionId={session.activeSessionId}
        workspace={activeWorkspace}
        models={models}
        selectedModelId={selectedModelId}
        selectedVariant={selectedModel?.variant ?? null}
        workspaceNames={workspaceNames}
        onClose={() => setCommandPaletteOpen(false)}
        onSelectSession={(s) => {
          if (session.isStreaming) return;
          void session.handleSessionSelect(s);
        }}
        onNewSession={() => {
          if (session.isStreaming) return;
          session.handleNewSession();
        }}
        onOpenSettings={openSettings}
        onOpenWorkspaceSwitcher={() => setWorkspaceSwitcherOpen(true)}
        onToggleSessions={toggleSessionDrawer}
        onToggleReview={toggleReviewPanel}
        onSelectModel={(modelId) => {
          applyModelSelection(modelId);
        }}
        onVariantChange={applyVariantSelection}
        restoreFocusRef={commandPaletteRestoreRef}
      />
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
