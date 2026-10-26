import { ArrowRight, Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Workspace } from "../types";
import { Button } from "./ui/button";
import { WorkspacePicker } from "./WorkspacePicker";

interface NewTaskFormProps {
  workspaces: Workspace[];
  activeWorkspaceId?: string;
  hasModel: boolean;
  onContinue: (workspaceId: string) => void;
  onCancel: () => void;
  onAddWorkspace: () => Promise<Workspace | null>;
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
  onAddWorkspace,
  onOpenSettings,
  creating,
  error,
}: NewTaskFormProps) {
  const [workspaceId, setWorkspaceId] = useState(activeWorkspaceId ?? "");
  const workspaceRef = useRef<HTMLButtonElement>(null);
  const [addingWorkspace, setAddingWorkspace] = useState(false);
  useEffect(() => {
    if (!addingWorkspace) workspaceRef.current?.focus();
  }, [addingWorkspace]);
  const workspace = workspaces.find((item) => item.id === workspaceId);
  const addWorkspace = async () => {
    if (addingWorkspace) return;
    setAddingWorkspace(true);
    try {
      const added = await onAddWorkspace();
      if (added) setWorkspaceId(added.id);
    } finally {
      setAddingWorkspace(false);
    }
  };
  return (
    <main className="new-task-page" aria-label="New task setup">
      <form
        className="new-task-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (workspace && hasModel && !creating && !addingWorkspace) onContinue(workspaceId);
        }}
      >
        <header className="new-task-heading">
          <h1>Where should we work?</h1>
          <p>Choose a workspace to start a new chat.</p>
        </header>
        <fieldset disabled={creating || addingWorkspace} className="new-task-fields">
          <div className="new-task-field">
            <WorkspacePicker
              workspaces={workspaces}
              value={workspaceId}
              onChange={setWorkspaceId}
              onAdd={() => {
                void addWorkspace();
              }}
              disabled={creating || addingWorkspace}
              triggerRef={workspaceRef}
            />
            <Button
              variant="ghost"
              size="sm"
              className="new-task-add"
              onClick={() => {
                void addWorkspace();
              }}
              disabled={addingWorkspace}
            >
              <Plus size={14} aria-hidden="true" />
              {addingWorkspace ? "Choosing folder…" : "Add workspace"}
            </Button>
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
            <Button variant="ghost" onClick={onCancel} disabled={creating || addingWorkspace}>
              Cancel
            </Button>
            <Button
              className="command-primary"
              type="submit"
              disabled={!workspace || !hasModel || creating || addingWorkspace}
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
