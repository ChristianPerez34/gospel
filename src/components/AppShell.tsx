import { useState, useCallback, useEffect, useRef } from "react";
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
import { useChatStream } from "../hooks/useChatStream";
import type {
  Message,
  Session,
  AgentStatus,
} from "../types";
import { noModelCopy } from "../modelAvailabilityCopy";

function modelOptionId(provider: string, model: string) {
  return `${provider.toLowerCase()}::${model}`;
}

export function AppShell() {
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [workspaceSwitcherOpen, setWorkspaceSwitcherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { workspaces, activeWorkspace, addWorkspace, removeWorkspace, switchWorkspace, loading: workspacesLoading } = useWorkspaces();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
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

  const [status, setStatus] = useState<AgentStatus>("idle");
  const statusRef = useRef(status);
  statusRef.current = status;

  useEffect(() => {
    if (statusRef.current !== "thinking") {
      setStatus(availabilitySnapshot?.available_models?.length ? "connected" : "idle");
    }
  }, [availabilitySnapshot]);

  useEffect(() => {
    if (!activeSessionId) return;

    setSessions((prev) =>
      prev.map((session) =>
        session.id === activeSessionId
          ? {
              ...session,
              messages,
              timestamp: messages[messages.length - 1]?.timestamp ?? session.timestamp,
            }
          : session
      )
    );
  }, [activeSessionId, messages]);

  const {
    streamingContent,
    toolActivities,
    isThinking,
    setIsThinking,
    startStream,
    resetStream,
  } = useChatStream({
    onMessages: setMessages,
    onStatusChange: setStatus,
    onErrorToast: showError,
    onSuccessToast: showSuccess,
    onOpenSettings: () => setSettingsOpen(true),
  });

  const handleSend = useCallback(async (message: string, invokedSkill?: { name: string; args?: string }) => {
    if (!selectedModel || !models.some((m) => m.name === selectedModel.model && m.provider.toLowerCase() === selectedModel.provider.toLowerCase())) {
      showError("Select an available model before sending.", {
        label: "Open Settings",
        onClick: () => setSettingsOpen(true),
      });
      return;
    }

    const userMsg: Message = {
      id: `m-${Date.now()}-user`,
      role: "user",
      content: invokedSkill ? `/${invokedSkill.name}${invokedSkill.args ? " " + invokedSkill.args : ""}` : message,
      timestamp: new Date(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setIsThinking(true);
    setStatus("thinking");
    resetStream();

    let effectiveSessionId = activeSessionId;
    if (!activeSessionId) {
      const newSession: Session = {
        id: `s-${Date.now()}`,
        title: userMsg.content.slice(0, 50) + (userMsg.content.length > 50 ? "..." : ""),
        provider: selectedModel.provider,
        model: selectedModel.model,
        timestamp: new Date(),
        messages: [userMsg],
        status: "active",
      };
      setSessions((prev) => [newSession, ...prev]);
      setActiveSessionId(newSession.id);
      effectiveSessionId = newSession.id;
    }

    try {
      await startStream({
        provider: selectedModel.provider,
        prompt: message,
        model: selectedModel.model,
        sessionId: effectiveSessionId,
        invokedSkill: invokedSkill ?? null,
      });
    } catch (e) {
      setIsThinking(false);
      setStatus("error");
      resetStream();
      showError(`Failed to send: ${e}`, {
        label: "Open Settings",
        onClick: () => setSettingsOpen(true),
      });
    }
  }, [activeSessionId, models, selectedModel, showError, resetStream, startStream, setIsThinking]);

  const handleSessionSelect = useCallback((session: Session) => {
    setActiveSessionId(session.id);
    setMessages(session.messages);
    setSessionDrawerOpen(false);
  }, []);

  const handleNewSession = useCallback(() => {
    setActiveSessionId(null);
    setMessages([]);
    resetStream();
    setSessionDrawerOpen(false);
  }, [resetStream]);

  const handleWorkspaceSwitcherClose = useCallback(() => {
    setWorkspaceSwitcherOpen(false);
  }, []);

  const activeSession = sessions.find((s) => s.id === activeSessionId);
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
        status={status}
        onSessionTitleChange={() => {}}
        onWorkspaceSwitch={() => setWorkspaceSwitcherOpen(true)}
        onToggleSessions={() => setSessionDrawerOpen(!sessionDrawerOpen)}
        onOpenSettings={() => setSettingsOpen(true)}
        sessionsOpen={sessionDrawerOpen}
      />
      <div className="flex flex-col flex-1 min-h-0 relative">
        <ChatView
          messages={messages}
          workspacePath={activeWorkspace?.path ?? ""}
          isThinking={isThinking}
          currentAction={streamingContent ? { type: "streaming" as const, content: streamingContent } : undefined}
          toolActivities={toolActivities}
        />
        <InputBar
          models={models}
          selectedModel={selectedModelId}
          onModelChange={(modelId) => {
            const match = models.find((m) => m.id === modelId);
            if (match) setSelectedModel({ provider: match.provider, model: match.name });
          }}
          onSend={handleSend}
          contextFiles={[]}
          onRemoveContext={() => {}}
          disabled={isThinking || models.length === 0}
          unavailableMessage={models.length === 0 ? noModels.title : "Connecting..."}
          unavailableDetail={noModels.detail}
          unavailableActionLabel={noModels.actionLabel}
          onUnavailableAction={() => setSettingsOpen(true)}
          workspacePath={activeWorkspace?.path}
        />
      </div>
      <SessionDrawer
        sessions={sessions}
        activeSessionId={activeSessionId ?? undefined}
        onSelect={handleSessionSelect}
        onNewSession={handleNewSession}
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
          onClose={handleWorkspaceSwitcherClose}
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
