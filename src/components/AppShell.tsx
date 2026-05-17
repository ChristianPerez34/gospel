import { useState, useCallback } from "react";
import { TopBar } from "./TopBar";
import { ChatView } from "./ChatView";
import { InputBar } from "./InputBar";
import { SessionDrawer } from "./SessionDrawer";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import type {
  Message,
  Session,
  Workspace,
  ModelOption,
  AgentStatus,
} from "../types";
import "./AppShell.css";

const DEMO_WORKSPACES: Workspace[] = [
  { id: "ws-1", name: "gospel", path: "~/Projects/gospel", sessionCount: 3 },
  { id: "ws-2", name: "my-api", path: "~/Projects/my-api", sessionCount: 1 },
  { id: "ws-3", name: "dotfiles", path: "~/.dotfiles", sessionCount: 0 },
];

const DEMO_MODELS: ModelOption[] = [
  { id: "claude-4-sonnet", name: "Claude 4 Sonnet", provider: "Anthropic" },
  { id: "gpt-4o", name: "GPT-4o", provider: "OpenAI" },
  { id: "gemini-2.5-pro", name: "Gemini 2.5 Pro", provider: "Google" },
];

const DEMO_MESSAGES: Message[] = [
  {
    id: "m-1",
    role: "user",
    content:
      "Review the Cargo.toml and suggest improvements for the dependency versions.",
    timestamp: new Date(Date.now() - 300000),
  },
  {
    id: "m-2",
    role: "agent",
    content:
      "I checked your Cargo.toml. Here is what I found:\n\n1. `tauri` is at v2, which is current. Good.\n2. `serde` and `serde_json` are both at v1, which is the latest major release.\n3. The `authors` field is set to `you`, which should be updated.\n\nThe dependencies look reasonable for a Tauri project. I will update the authors field.",
    timestamp: new Date(Date.now() - 290000),
    actionCards: [
      {
        id: "ac-1",
        type: "file",
        summary: "Read Cargo.toml",
        content: 'name = "gospel"\nversion = "0.1.0"\nauthors = ["you"]',
        expanded: false,
      },
      {
        id: "ac-2",
        type: "diff",
        summary: 'Edited authors field in Cargo.toml',
        content:
          '- authors = ["you"]\n+ authors = ["Christian Perez"]',
        expanded: false,
      },
    ],
  },
  {
    id: "m-3",
    role: "user",
    content: "Good. Now run the tests.",
    timestamp: new Date(Date.now() - 240000),
  },
  {
    id: "m-4",
    role: "agent",
    content: "Tests pass. All 12 tests in 0.8s.",
    timestamp: new Date(Date.now() - 230000),
    actionCards: [
      {
        id: "ac-3",
        type: "terminal",
        summary: "Ran `cargo test`",
        content: "running 12 tests\ntest result: ok. 12 passed; 0 failed; 0 ignored\n\nFinished in 0.82s",
        expanded: false,
      },
    ],
  },
];

const DEMO_SESSIONS: Session[] = [
  {
    id: "s-1",
    title: "Review Cargo.toml dependencies",
    model: "Claude 4 Sonnet",
    timestamp: new Date(),
    messages: DEMO_MESSAGES,
    status: "active",
  },
  {
    id: "s-2",
    title: "Fix TypeScript errors in App",
    model: "GPT-4o",
    timestamp: new Date(Date.now() - 86400000),
    messages: [],
    status: "idle",
  },
  {
    id: "s-3",
    title: "Add Tauri window management",
    model: "Claude 4 Sonnet",
    timestamp: new Date(Date.now() - 172800000),
    messages: [],
    status: "idle",
  },
];

export function AppShell() {
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [workspaceSwitcherOpen, setWorkspaceSwitcherOpen] = useState(false);
  const [activeWorkspace] = useState(DEMO_WORKSPACES[0]);
  const [sessionTitle] = useState("Review Cargo.toml dependencies");
  const [selectedModel, setSelectedModel] = useState("claude-4-sonnet");
  const [status] = useState<AgentStatus>("connected");

  const handleSend = useCallback((message: string) => {
    console.log("Send message:", message);
  }, []);

  const handleSessionSelect = useCallback((session: Session) => {
    console.log("Select session:", session.id);
  }, []);

  const handleNewSession = useCallback(() => {
    console.log("New session");
  }, []);

  return (
    <div className="app-shell" data-theme="dark">
      <TopBar
        workspace={activeWorkspace}
        sessionTitle={sessionTitle}
        model={DEMO_MODELS.find((m) => m.id === selectedModel)?.name || ""}
        status={status}
        onSessionTitleChange={() => {}}
        onWorkspaceSwitch={() => setWorkspaceSwitcherOpen(true)}
        onToggleSessions={() => setSessionDrawerOpen(!sessionDrawerOpen)}
        sessionsOpen={sessionDrawerOpen}
      />
      <div className="app-shell__body">
        <ChatView
          messages={DEMO_MESSAGES}
          workspacePath={activeWorkspace.path}
          isThinking={false}
          currentAction={undefined}
        />
        <InputBar
          models={DEMO_MODELS}
          selectedModel={selectedModel}
          onModelChange={setSelectedModel}
          onSend={handleSend}
          contextFiles={[
            { name: "Cargo.toml", path: "Cargo.toml" },
            { name: "src/", path: "src/" },
          ]}
          onRemoveContext={() => {}}
        />
      </div>
      <SessionDrawer
        sessions={DEMO_SESSIONS}
        activeSessionId="s-1"
        onSelect={handleSessionSelect}
        onNewSession={handleNewSession}
        onClose={() => setSessionDrawerOpen(false)}
        open={sessionDrawerOpen}
      />
      {workspaceSwitcherOpen && (
        <WorkspaceSwitcher
          workspaces={DEMO_WORKSPACES}
          activeWorkspaceId={activeWorkspace.id}
          onSelect={() => {}}
          onAdd={() => {}}
          onClose={() => setWorkspaceSwitcherOpen(false)}
        />
      )}
    </div>
  );
}