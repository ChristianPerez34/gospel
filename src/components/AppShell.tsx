import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TopBar } from "./TopBar";
import { ChatView } from "./ChatView";
import { InputBar } from "./InputBar";
import { SessionDrawer } from "./SessionDrawer";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { SettingsModal } from "./SettingsModal";
import { ToastContainer, useToasts } from "./Toast";
import { useWorkspaces } from "../hooks/useWorkspaces";
import type {
  Message,
  Session,
  ModelOption,
  AgentStatus,
} from "../types";
import type { ProviderConfig, ProviderId } from "./ProviderSelector";
import { noModelCopy } from "../modelAvailabilityCopy";
import "./AppShell.css";

interface ProviderAvailability {
  provider: ProviderId;
  display_name: string;
  auth_type: "api_key" | "oauth";
  credentialed: boolean;
  visible: boolean;
  model_fetch_status: string;
  model_count: number;
  error_kind?: string | null;
  error_detail?: string | null;
}

interface ModelAvailabilitySnapshot {
  providers: ProviderAvailability[];
  available_models: { model: string; provider: string }[];
  empty_reason?: string | null;
  warnings: string[];
}

interface SelectedModel {
  provider: string;
  model: string;
}

function modelOptionId(provider: string, model: string) {
  return `${provider.toLowerCase()}::${model}`;
}

function providerConfigFromAvailability(provider: ProviderAvailability, existing?: ProviderConfig): ProviderConfig {
  return {
    id: provider.provider,
    name: provider.display_name,
    authType: provider.auth_type,
    credentialed: provider.credentialed,
    visible: provider.visible,
    modelFetchStatus: provider.model_fetch_status,
    modelCount: provider.model_count,
    errorKind: provider.error_kind ?? undefined,
    errorDetail: provider.error_detail ?? undefined,
    apiKey: provider.credentialed ? "" : existing?.apiKey ?? "",
    enabled: provider.visible,
    status: existing?.status ?? (provider.credentialed ? "success" : "idle"),
    testMessage: existing?.testMessage ?? "",
    isOAuth: provider.auth_type === "oauth",
    isAuthenticated: provider.auth_type === "oauth" ? provider.credentialed : undefined,
  };
}

function buildModelOptions(models: { model: string; provider: string }[], providers: ProviderConfig[]): ModelOption[] {
  return models.map((m) => {
    const provider = providers.find((p) => p.id === m.provider.toLowerCase() as ProviderId);
    return {
      id: modelOptionId(m.provider, m.model),
      name: m.model,
      provider: m.provider,
      configured: provider?.credentialed ?? true,
    };
  });
}

export function AppShell() {
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [workspaceSwitcherOpen, setWorkspaceSwitcherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { workspaces, activeWorkspace, addWorkspace, removeWorkspace, switchWorkspace, loading: workspacesLoading } = useWorkspaces();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [selectedModel, setSelectedModel] = useState<SelectedModel | null>(null);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [availabilitySnapshot, setAvailabilitySnapshot] = useState<ModelAvailabilitySnapshot | null>(null);
  const [isRefreshingModels, setIsRefreshingModels] = useState(false);
  const [status, setStatus] = useState<AgentStatus>("idle");
  const statusRef = useRef(status);
  statusRef.current = status;
  const [isThinking, setIsThinking] = useState(false);
  const [streamingContent, setStreamingContent] = useState("");
  const { toasts, dismissToast, showError, showSuccess } = useToasts();
  const unlistenRef = useRef<(() => void) | null>(null);
  const providersRef = useRef(providers);
  providersRef.current = providers;

  const [availableModels, setAvailableModels] = useState<{ model: string; provider: string }[]>([]);
  const isRefreshingModelsRef = useRef(false);

  const refreshModelAvailability = useCallback(async (forceRefresh = false) => {
    if (forceRefresh && isRefreshingModelsRef.current) return;
    if (forceRefresh) {
      setIsRefreshingModels(true);
      isRefreshingModelsRef.current = true;
    }
    try {
      const snapshot = await invoke<ModelAvailabilitySnapshot>("get_model_availability", { forceRefresh });
      setAvailabilitySnapshot(snapshot);
      setAvailableModels(snapshot.available_models);
      setProviders((current) =>
        snapshot.providers.map((provider) =>
          providerConfigFromAvailability(provider, current.find((p) => p.id === provider.provider))
        )
      );
      if (statusRef.current !== "thinking") {
        setStatus(snapshot.available_models.length > 0 ? "connected" : "idle");
      }
      if (forceRefresh) {
        const failedProvider = snapshot.providers.find((p) => p.error_kind || p.model_fetch_status === "failed");
        if (failedProvider) {
          showError(`${failedProvider.display_name}: ${failedProvider.error_detail || "Model refresh failed."}`);
        } else {
          showSuccess("Models refreshed.");
        }
      }
    } catch (e) {
      if (forceRefresh) {
        showError(`Model refresh failed: ${e}`);
      } else {
        setAvailabilitySnapshot(null);
        if (statusRef.current !== "thinking") {
          setStatus("idle");
        }
      }
    } finally {
      if (forceRefresh) {
        setIsRefreshingModels(false);
        isRefreshingModelsRef.current = false;
      }
    }
  }, [showError, showSuccess]);

  useEffect(() => {
    void refreshModelAvailability();
  }, [refreshModelAvailability]);

  useEffect(() => {
    const models = buildModelOptions(availableModels, providers);
    setModels(models);
    if (models.length === 0 || availableModels.length === 0) {
      setSelectedModel(null);
      return;
    }
    setSelectedModel((prev) => {
      if (prev && availableModels.some((m) => m.model === prev.model && m.provider.toLowerCase() === prev.provider.toLowerCase())) {
        return prev;
      }
      return { provider: availableModels[0].provider, model: availableModels[0].model };
    });
  }, [availableModels, providers]);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      const unlistenToken = await listen<string>("llm-token", (event) => {
        setStreamingContent((prev) => prev + event.payload);
        setIsThinking(false);
      });

      const unlistenDone = await listen<string>("llm-done", (event) => {
        const content = event.payload;
        if (content) {
          setMessages((prev) => [
            ...prev,
            {
              id: `m-${Date.now()}`,
              role: "agent",
              content,
              timestamp: new Date(),
            },
          ]);
        }
        setStreamingContent("");
        setIsThinking(false);
        setStatus("connected");
      });

      const unlistenError = await listen<{ code: string; message: string }>("llm-error", (event) => {
        const err = event.payload;
        setIsThinking(false);
        setStreamingContent("");
        setStatus("error");

        if (err?.code === "API_KEY_MISSING") {
          showError(err.message, {
            label: "Open Settings",
            onClick: () => setSettingsOpen(true),
          });
        } else {
          showError(err?.message || "Completion failed.", {
            label: "Retry",
            onClick: () => {},
          });
        }
      });

      if (cancelled) {
        unlistenToken();
        unlistenDone();
        unlistenError();
        return;
      }

      unlistenRef.current = () => {
        unlistenToken();
        unlistenDone();
        unlistenError();
      };
    })();

    return () => {
      cancelled = true;
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showError]);

  const handleSend = useCallback(async (message: string) => {
    if (!selectedModel || !availableModels.some((m) => m.model === selectedModel.model && m.provider.toLowerCase() === selectedModel.provider.toLowerCase())) {
      showError("Select an available model before sending.", {
        label: "Open Settings",
        onClick: () => setSettingsOpen(true),
      });
      return;
    }

    const userMsg: Message = {
      id: `m-${Date.now()}-user`,
      role: "user",
      content: message,
      timestamp: new Date(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setIsThinking(true);
    setStatus("thinking");
    setStreamingContent("");

    if (!activeSessionId) {
      const newSession: Session = {
        id: `s-${Date.now()}`,
        title: message.slice(0, 50) + (message.length > 50 ? "..." : ""),
        provider: selectedModel.provider,
        model: selectedModel.model,
        timestamp: new Date(),
        messages: [userMsg],
        status: "active",
      };
      setSessions((prev) => [newSession, ...prev]);
      setActiveSessionId(newSession.id);
    }

    try {
      await invoke("complete_streaming", {
        provider: selectedModel.provider,
        prompt: message,
        model: selectedModel.model,
      });
    } catch (e) {
      setIsThinking(false);
      setStatus("error");
      setStreamingContent("");
      showError(`Failed to send: ${e}`, {
        label: "Open Settings",
        onClick: () => setSettingsOpen(true),
      });
    }
  }, [activeSessionId, availableModels, selectedModel, showError]);

  const handleSessionSelect = useCallback((session: Session) => {
    setActiveSessionId(session.id);
    setMessages(session.messages);
    setSessionDrawerOpen(false);
  }, []);

  const handleNewSession = useCallback(() => {
    setActiveSessionId(null);
    setMessages([]);
    setStreamingContent("");
    setIsThinking(false);
    setSessionDrawerOpen(false);
  }, []);

  const handleWorkspaceSwitcherClose = useCallback(() => {
    setWorkspaceSwitcherOpen(false);
  }, []);

  const activeSession = sessions.find((s) => s.id === activeSessionId);
  const sessionTitle = activeSession?.title || "New session";
  const selectedModelId = selectedModel ? modelOptionId(selectedModel.provider, selectedModel.model) : "";
  const currentModelName = selectedModel?.model || "No model";
  const noModels = noModelCopy(availabilitySnapshot);

  return (
    <div className="app-shell" data-theme="dark">
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
      <div className="app-shell__body">
        <ChatView
          messages={messages}
          workspacePath={activeWorkspace?.path ?? ""}
          isThinking={isThinking}
          currentAction={streamingContent ? { type: "streaming" as const, content: streamingContent } : undefined}
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
