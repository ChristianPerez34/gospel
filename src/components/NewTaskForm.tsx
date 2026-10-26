import { ArrowRight, Folder } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Workspace } from "../types";
import { Button } from "./ui/button";

interface NewTaskFormProps {
  workspaces: Workspace[];
  activeWorkspaceId?: string;
  hasModel: boolean;
  onContinue: (workspaceId: string) => void;
  onCancel: () => void;
  onManageWorkspaces: () => void;
  onOpenSettings: () => void;
  creating: boolean;
  error: string | null;
}

export function NewTaskForm({
  workspaces,
  activeWorkspaceId,
  hasModel,
  onContinue,
  onCancel,
  onManageWorkspaces,
  onOpenSettings,
  creating,
  error,
}: NewTaskFormProps) {
  const [workspaceId, setWorkspaceId] = useState(activeWorkspaceId ?? "");
  const workspaceRef = useRef<HTMLSelectElement>(null);
  useEffect(() => {
    workspaceRef.current?.focus();
  }, []);
  const workspace = workspaces.find((item) => item.id === workspaceId);
  return (
    <main className="new-task-page" aria-label="New task setup">
      <form
        className="new-task-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (workspace && hasModel && !creating) onContinue(workspaceId);
        }}
      >
        <header className="new-task-heading">
          <h1>Where should we work?</h1>
          <p>Choose a workspace to start a new chat.</p>
        </header>
        <fieldset disabled={creating} className="new-task-fields">
          <div className="new-task-field">
            <label htmlFor="task-workspace">Workspace</label>
            <select
              ref={workspaceRef}
              id="task-workspace"
              value={workspaceId}
              onChange={(event) => setWorkspaceId(event.target.value)}
              required
              aria-describedby="task-workspace-path"
            >
              <option value="">Choose a workspace</option>
              {workspaces.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
            <div className="new-task-workspace-path" id="task-workspace-path">
              <Folder size={13} aria-hidden="true" />
              <span>
                {workspace?.path ?? "Select the project directory for this conversation."}
              </span>
            </div>
            {workspaces.length === 0 && (
              <Button variant="outline" onClick={onManageWorkspaces}>
                Add a workspace
              </Button>
            )}
          </div>
          {!hasModel && (
            <p className="new-task-recovery">
              Connect a model to start chatting.{" "}
              <Button variant="link" size="sm" onClick={onOpenSettings}>
                Model settings
              </Button>
            </p>
          )}
        </fieldset>
        {error && (
          <p role="alert" className="new-task-error">
            {error}
          </p>
        )}
        <footer className="new-task-footer">
          <p>The title is generated automatically from your conversation.</p>
          <div>
            <Button variant="ghost" onClick={onCancel} disabled={creating}>
              Cancel
            </Button>
            <Button
              className="command-primary"
              type="submit"
              disabled={!workspace || !hasModel || creating}
            >
              {creating ? "Opening chat…" : "Continue to chat"}
              <ArrowRight size={14} aria-hidden="true" />
            </Button>
          </div>
        </footer>
      </form>
    </main>
  );
}
