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
import { modelOptionId, type Session } from "../types";
import { noModelCopy } from "../modelAvailabilityCopy";

type SettingsTab = "general" | "models";
type TrappedSurface = "sessions" | "review" | null;

interface BackendSessionRecord {
  id: string;
  title: string;
  provider: string;
  model: string;
  status: string;
  workspace_id: string | null;
  updated_at: string;
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

  const session = useSessionManager({
    models,
    selectedModel,
    activeWorkspaceId: activeWorkspace?.id,
    onSwitchWorkspace: switchWorkspace,
    onError: showError,
    onSuccess: showSuccess,
    onOpenSettings: openSettings,
  });

  const [allWorkspaceSessions, setAllWorkspaceSessions] = useState<Session[]>([]);

  const workspaceNames = useMemo<Record<string, string>>(() => {
    const names: Record<string, string> = {};
    for (const item of workspaces) {
      names[item.id] = item.name;
    }
    return names;
  }, [workspaces]);

  const mapBackendSessions = useCallback((backendSessions: BackendSessionRecord[]) => {
    return backendSessions.map((session) => ({
      id: session.id,
      title: session.title,
      provider: session.provider,
      model: session.model,
      timestamp: new Date(session.updated_at),
      messages: [],
      status: (session.status === "active" ? "idle" : "error") as Session["status"],
      workspaceId: session.workspace_id ?? undefined,
    }));
  }, []);

  useEffect(() => {
    let cancelled = false;

    const loadWorkspaceSessions = async () => {
      const ids = workspaces.map((item) => item.id);
      if (ids.length === 0) {
        if (!cancelled) {
          setAllWorkspaceSessions([]);
        }
        return;
      }

      const workspaceSessions = await Promise.all(
        ids.map((id) => invoke<BackendSessionRecord[]>("list_sessions", { workspace_id: id })),
      );

      if (cancelled) return;

      const flattened = workspaceSessions.flatMap((backendSessions) => mapBackendSessions(backendSessions));
      setAllWorkspaceSessions(flattened);
    };

    void loadWorkspaceSessions();

    return () => {
      cancelled = true;
    };
  }, [mapBackendSessions, workspaces]);

  const allSessions = useMemo(() => {
    const byId = new Map<string, Session>();

    for (const item of allWorkspaceSessions) {
      byId.set(item.id, item);
    }

    for (const item of session.sessions) {
      byId.set(item.id, item);
    }

    return Array.from(byId.values()).sort(
      (left, right) => right.timestamp.getTime() - left.timestamp.getTime(),
    );
  }, [allWorkspaceSessions, session.sessions]);

  const activeSession = session.sessions.find((s) => s.id === session.activeSessionId);
  const sessionTitle = activeSession?.title || "New session";
  const selectedModelId = selectedModel ? modelOptionId(selectedModel.provider, selectedModel.model) : "";
  const currentModelName = selectedModel?.model || "No model";
  const noModels = noModelCopy(availabilitySnapshot);
  const surfaceTrapOpen = trappedSurface !== null;
  const modalSurfaceOpen = commandPaletteOpen || settingsOpen || workspaceSwitcherOpen;

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
              finalizedToolActivities={session.finalizedToolActivities}
            />
            <InputBar
              models={models}
              selectedModel={selectedModelId}
              onModelChange={(modelId) => {
                const match = models.find((m) => m.id === modelId);
                if (match) setSelectedModel({ provider: match.provider, model: match.name });
              }}
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
          activeSessionId={session.activeSessionId ?? undefined}
          workspaceNames={workspaceNames}
          activeWorkspaceId={activeWorkspace?.id}
          onSelect={(s) => {
            if (session.isStreaming) return;
            void session.handleSessionSelect(s);
            closeSessionDrawer();
          }}
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
      />
      <CommandPalette
        open={commandPaletteOpen}
        sessions={allSessions}
        activeSessionId={session.activeSessionId}
        workspace={activeWorkspace}
        models={models}
        selectedModelId={selectedModelId}
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
          const match = models.find((m) => m.id === modelId);
          if (match) setSelectedModel({ provider: match.provider, model: match.name });
        }}
        restoreFocusRef={commandPaletteRestoreRef}
      />
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
