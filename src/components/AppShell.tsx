import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TopBar } from "./TopBar";
import { ChatView } from "./ChatView";
import { InputBar } from "./InputBar";
import { SessionDrawer } from "./SessionDrawer";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { SettingsModal } from "./SettingsModal";
import { ToastContainer, useToasts } from "./Toast";
import { useWorkspaces } from "../hooks/useWorkspaces";
import { useModelAvailability } from "../hooks/useModelAvailability";
import { useSessionManager } from "../hooks/useSessionManager";
import { modelOptionId } from "../types";
import { noModelCopy } from "../modelAvailabilityCopy";

export function AppShell() {
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [workspaceSwitcherOpen, setWorkspaceSwitcherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const openSettings = useCallback(() => setSettingsOpen(true), []);

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
    onError: showError,
    onSuccess: showSuccess,
    onOpenSettings: openSettings,
  });

  const activeSession = session.sessions.find((s) => s.id === session.activeSessionId);
  const sessionTitle = activeSession?.title || "New session";
  const selectedModelId = selectedModel ? modelOptionId(selectedModel.provider, selectedModel.model) : "";
  const currentModelName = selectedModel?.model || "No model";
  const noModels = noModelCopy(availabilitySnapshot);

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-surface-base text-text-primary" data-theme="dark">
      <TopBar
        workspace={activeWorkspace ?? { id: "", name: "No workspace", path: "", sessionCount: 0 }}
        sessionTitle={sessionTitle}
        model={currentModelName}
        status={session.status}
        onWorkspaceSwitch={() => setWorkspaceSwitcherOpen(true)}
        onToggleSessions={() => setSessionDrawerOpen(!sessionDrawerOpen)}
        onOpenSettings={openSettings}
        sessionsOpen={sessionDrawerOpen}
      />
      <div className="flex flex-col flex-1 min-h-0 relative">
        <ChatView
          messages={session.messages}
          workspacePath={activeWorkspace?.path ?? ""}
          isThinking={session.isStreaming}
          currentAction={session.streamingContent ? { type: "streaming" as const, content: session.streamingContent } : undefined}
          toolActivities={session.toolActivities}
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
          onUnavailableAction={openSettings}
          workspacePath={activeWorkspace?.path}
        />
      </div>
      <SessionDrawer
        sessions={session.sessions}
        activeSessionId={session.activeSessionId ?? undefined}
        onSelect={(s) => {
          if (session.isStreaming) return;
          session.handleSessionSelect(s);
          setSessionDrawerOpen(false);
        }}
        onNewSession={() => {
          if (session.isStreaming) return;
          session.handleNewSession();
          setSessionDrawerOpen(false);
        }}
        onClose={() => setSessionDrawerOpen(false)}
        open={sessionDrawerOpen}
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
        />
      )}
      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        providers={providers}
        onProvidersChange={setProviders}
        onRefreshAvailability={refreshModelAvailability}
        isRefreshingModels={isRefreshingModels}
      />
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
