import { ArrowUpRight, Folder } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ModelOption, Workspace } from "../types";
import { Button } from "./ui/button";

interface NewTaskFormProps {
  workspaces: Workspace[];
  activeWorkspaceId?: string;
  models: ModelOption[];
  selectedModelId: string;
  selectedVariant: string | null;
  onModelChange: (id: string) => void;
  onVariantChange: (variant: string | null) => void;
  onStart: (title: string, workspaceId: string, instruction: string) => void;
  onCancel: () => void;
  onManageWorkspaces: () => void;
  onOpenSettings: () => void;
  creating: boolean;
  error: string | null;
}

export function NewTaskForm({
  workspaces,
  activeWorkspaceId,
  models,
  selectedModelId,
  selectedVariant,
  onModelChange,
  onVariantChange,
  onStart,
  onCancel,
  onManageWorkspaces,
  onOpenSettings,
  creating,
  error,
}: NewTaskFormProps) {
  const [workspaceId, setWorkspaceId] = useState(activeWorkspaceId ?? "");
  const [title, setTitle] = useState("");
  const [instruction, setInstruction] = useState("");
  const titleRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    titleRef.current?.focus();
  }, []);
  const workspace = workspaces.find((item) => item.id === workspaceId);
  const model = models.find((item) => item.id === selectedModelId && item.configured !== false);
  const variants = model?.variants ?? [];
  const variantAvailable =
    !selectedVariant || variants.some((variant) => variant.id === selectedVariant);
  const canStart = Boolean(
    workspace && model && variantAvailable && title.trim() && instruction.trim()
  );

  return (
    <main className="new-task-page" aria-label="New task setup">
      <form
        className="new-task-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (canStart && !creating) onStart(title.trim(), workspaceId, instruction.trim());
        }}
      >
        <header className="new-task-heading">
          <span className="command-center-eyebrow">TASK SETUP</span>
          <h1>New task</h1>
          <p>Choose where Gospel works and what it should do.</p>
        </header>
        <fieldset disabled={creating} className="new-task-fields">
          <div className="new-task-field">
            <label htmlFor="task-workspace">Workspace</label>
            <select
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
              <span>{workspace?.path ?? "Select the project directory this task belongs to."}</span>
            </div>
            {workspaces.length === 0 && (
              <Button variant="outline" onClick={onManageWorkspaces}>
                Add a workspace
              </Button>
            )}
          </div>
          <div className="new-task-field">
            <label htmlFor="task-name">Task name</label>
            <input
              ref={titleRef}
              id="task-name"
              placeholder="e.g. Fix the authentication redirect loop"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              required
            />
          </div>
          <div className="new-task-field">
            <label htmlFor="task-instruction">Instruction</label>
            <textarea
              id="task-instruction"
              placeholder="Describe the outcome, scope, and constraints…"
              value={instruction}
              onChange={(event) => setInstruction(event.target.value)}
              rows={6}
              required
              aria-describedby="task-instruction-hint"
            />
            <p id="task-instruction-hint">
              Tell the agent what to do, what success looks like, and anything it should leave
              alone.
            </p>
          </div>
          <div className="new-task-models">
            <div className="new-task-field">
              <label htmlFor="task-model">Model</label>
              <select
                id="task-model"
                value={selectedModelId}
                onChange={(event) => onModelChange(event.target.value)}
                required
              >
                <option value="">Choose a model</option>
                {models.map((item) => (
                  <option key={item.id} value={item.id} disabled={item.configured === false}>
                    {item.name}
                  </option>
                ))}
              </select>
            </div>
            {(variants.length > 0 || selectedVariant) && (
              <div className="new-task-field">
                <label htmlFor="task-variant">Variant</label>
                <select
                  id="task-variant"
                  value={selectedVariant ?? ""}
                  onChange={(event) => onVariantChange(event.target.value || null)}
                >
                  <option value="">Default</option>
                  {variants.map((variant) => (
                    <option key={variant.id} value={variant.id}>
                      {variant.name}
                    </option>
                  ))}
                  {!variantAvailable && (
                    <option value={selectedVariant ?? ""} disabled>
                      Unavailable variant
                    </option>
                  )}
                </select>
              </div>
            )}
          </div>
          {!model && (
            <p className="new-task-recovery">
              {models.length === 0
                ? "Connect a model to start this task."
                : "Select an available model."}{" "}
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
          <p>
            The task is saved in {workspace?.name ?? "your selected workspace"} before the agent
            starts.
          </p>
          <div>
            <Button variant="ghost" onClick={onCancel} disabled={creating}>
              Cancel
            </Button>
            <Button className="command-primary" type="submit" disabled={!canStart || creating}>
              {creating ? "Creating task…" : "Create and start task"}
              <ArrowUpRight size={14} aria-hidden="true" />
            </Button>
          </div>
        </footer>
      </form>
    </main>
  );
}
