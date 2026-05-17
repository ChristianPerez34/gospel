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
import type {
  Message,
  Session,
  Workspace,
  ModelOption,
  AgentStatus,
} from "../types";
import "./AppShell.css";

const FALLBACK_WORKSPACES: Workspace[] = [
  { id: "ws-1", name: "gospel", path: "~/Projects/gospel", sessionCount: 0 },
];

function buildModelOptions(models: { model: string; provider: string }[]): ModelOption[] {
  return models.map((m) => ({
    id: m.model,
    name: m.model,
    provider: m.provider,
    configured: true,
  }));
}

export function AppShell() {
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [workspaceSwitcherOpen, setWorkspaceSwitcherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activeWorkspace] = useState(FALLBACK_WORKSPACES[0]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [selectedModel, setSelectedModel] = useState("gpt-4o");
  const [selectedProvider, setSelectedProvider] = useState("openai");
  const [models, setModels] = useState<ModelOption[]>([]);
  const [status, setStatus] = useState<AgentStatus>("idle");
  const [isThinking, setIsThinking] = useState(false);
  const [streamingContent, setStreamingContent] = useState("");
  const { toasts, dismissToast, showError, showSuccess: _showSuccess } = useToasts();
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const availableModels = await invoke<{ model: string; provider: string }[]>("get_available_models");
        if (availableModels.length > 0) {
          setModels(buildModelOptions(availableModels));
          setSelectedModel(availableModels[0].model);
          setSelectedProvider(availableModels[0].provider);
          setStatus("connected");
        } else {
          setStatus("idle");
        }
      } catch {
        setStatus("idle");
      }
    })();

    (async () => {
      const unlistenToken = await listen<string>("llm-token", (event) => {
        setStreamingContent((prev) => prev + event.payload);
        setIsThinking(false);
      });

      const unlistenDone = await listen<string>("llm-done", (event) => {
        const content = event.payload || streamingContent;
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

      unlistenRef.current = () => {
        unlistenToken();
        unlistenDone();
        unlistenError();
      };
    })();

    return () => {
      unlistenRef.current?.();
    };
  }, [showError, streamingContent]);

  const handleSend = useCallback(async (message: string) => {
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
        model: selectedModel,
        timestamp: new Date(),
        messages: [userMsg],
        status: "active",
      };
      setSessions((prev) => [newSession, ...prev]);
      setActiveSessionId(newSession.id);
    }

    try {
      await invoke("complete_streaming", {
        provider: selectedProvider,
        prompt: message,
        model: selectedModel,
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
  }, [activeSessionId, selectedModel, selectedProvider, showError]);

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

  const activeSession = sessions.find((s) => s.id === activeSessionId);
  const sessionTitle = activeSession?.title || "New session";
  const currentModelName = models.find((m) => m.id === selectedModel)?.name || selectedModel;

  return (
    <div className="app-shell" data-theme="dark">
      <TopBar
        workspace={activeWorkspace}
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
          workspacePath={activeWorkspace.path}
          isThinking={isThinking}
          currentAction={streamingContent ? { type: "streaming" as const, content: streamingContent } : undefined}
        />
        <InputBar
          models={models}
          selectedModel={selectedModel}
          onModelChange={(modelId) => {
            setSelectedModel(modelId);
            const match = models.find((m) => m.id === modelId);
            if (match?.provider) setSelectedProvider(match.provider.toLowerCase());
          }}
          onSend={handleSend}
          contextFiles={[]}
          onRemoveContext={() => {}}
          disabled={isThinking}
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
          workspaces={FALLBACK_WORKSPACES}
          activeWorkspaceId={activeWorkspace.id}
          onSelect={() => {}}
          onAdd={() => {}}
          onClose={() => setWorkspaceSwitcherOpen(false)}
        />
      )}
      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
      />
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
