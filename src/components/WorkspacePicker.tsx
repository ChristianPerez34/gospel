import { ChevronsUpDown, Folder, Plus } from "lucide-react";
import { type RefObject, useId, useState } from "react";
import type { Workspace } from "../types";
import { Button } from "./ui/button";
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxLabel,
  ComboboxList,
  ComboboxTrigger,
} from "./ui/combobox";

export function WorkspaceDetails({
  workspace,
  descriptionId,
}: {
  workspace: Workspace;
  descriptionId?: string;
}) {
  return (
    <>
      <span className="workspace-folder">
        <Folder size={18} strokeWidth={1.5} aria-hidden="true" />
      </span>
      <span className="workspace-details">
        <span className="workspace-name">{workspace.name}</span>
        <span className="workspace-path" id={descriptionId} title={workspace.path}>
          {workspace.path}
        </span>
      </span>
    </>
  );
}

interface WorkspacePickerProps {
  workspaces: Workspace[];
  value?: string;
  onChange: (id: string) => void;
  onAdd: () => void;
  disabled?: boolean;
  triggerRef?: RefObject<HTMLButtonElement>;
}
export function WorkspacePicker({
  workspaces,
  value,
  onChange,
  onAdd,
  disabled,
  triggerRef,
}: WorkspacePickerProps) {
  const [open, setOpen] = useState(false);
  const descriptionId = useId();
  const workspace = workspaces.find((item) => item.id === value) ?? null;
  return (
    <Combobox<Workspace>
      items={workspaces}
      value={workspace}
      onValueChange={(item) => {
        if (item) onChange(item.id);
      }}
      open={open}
      onOpenChange={setOpen}
      disabled={disabled}
      autoHighlight
      itemToStringLabel={(item) => item.name}
      itemToStringValue={(item) => item.id}
      isItemEqualToValue={(item, selected) => item.id === selected.id}
      filter={(item, query) =>
        `${item.name} ${item.path}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
      }
    >
      <ComboboxLabel className="workspace-picker-label">Workspace</ComboboxLabel>
      <ComboboxTrigger
        ref={triggerRef}
        className="workspace-picker-trigger"
        aria-describedby={workspace ? descriptionId : undefined}
      >
        {workspace ? (
          <WorkspaceDetails workspace={workspace} descriptionId={descriptionId} />
        ) : (
          <>
            <span className="workspace-folder">
              <Folder size={18} aria-hidden="true" />
            </span>
            <span className="workspace-details">
              <span className="workspace-name">Choose a workspace</span>
              <span className="workspace-path">Select a project folder</span>
            </span>
          </>
        )}
        <ChevronsUpDown size={15} aria-hidden="true" className="workspace-picker-chevron" />
      </ComboboxTrigger>
      <ComboboxContent>
        <ComboboxInput aria-label="Search workspaces" placeholder="Search by name or folder…" />
        <ComboboxEmpty>
          {workspaces.length === 0
            ? "Add a project folder to get started."
            : "No workspaces found."}
        </ComboboxEmpty>
        <ComboboxList>
          {(item: Workspace) => (
            <ComboboxItem key={item.id} value={item}>
              <WorkspaceDetails workspace={item} />
            </ComboboxItem>
          )}
        </ComboboxList>
        <div className="workspace-picker-footer">
          <Button
            variant="ghost"
            onClick={() => {
              setOpen(false);
              onAdd();
            }}
          >
            <Plus size={15} aria-hidden="true" />
            Add workspace
          </Button>
        </div>
      </ComboboxContent>
    </Combobox>
  );
}
