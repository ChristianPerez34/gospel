import { useState, useRef, useCallback } from "react";
import type { ModelOption } from "../types";
import { ContextPill } from "./ContextPill";

interface ContextFile {
  name: string;
  path: string;
}

interface InputBarProps {
  models: ModelOption[];
  selectedModel: string;
  onModelChange: (modelId: string) => void;
  onSend: (message: string) => void;
  contextFiles: ContextFile[];
  onRemoveContext: (path: string) => void;
  disabled?: boolean;
  unavailableMessage?: string;
  unavailableDetail?: string;
  unavailableActionLabel?: string;
  onUnavailableAction?: () => void;
}

export function InputBar({
  models,
  selectedModel,
  onModelChange,
  onSend,
  contextFiles,
  onRemoveContext,
  disabled = false,
  unavailableMessage = "No available models",
  unavailableDetail,
  unavailableActionLabel = "Open Settings",
  onUnavailableAction,
}: InputBarProps) {
  const [value, setValue] = useState("");
  const [modelOpen, setModelOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const currentModel = models.find((m) => m.id === selectedModel);
  const noModels = models.length === 0;
  const sendDisabled = disabled || noModels;

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        if (value.trim() && !sendDisabled) {
          onSend(value.trim());
          setValue("");
          if (textareaRef.current) {
            textareaRef.current.style.height = "auto";
          }
        }
      }
    },
    [value, sendDisabled, onSend]
  );

  const handleInput = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setValue(e.target.value);
    const el = e.target;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 200) + "px";
  }, []);

  return (
    <div className="bg-surface-elevated border-t border-surface-overlay flex flex-col shrink-0 z-[--z-sticky-input]">
      {contextFiles.length > 0 && (
        <div className="flex gap-1 pt-2 px-3 overflow-x-auto flex-nowrap">
          {contextFiles.map((file) => (
            <ContextPill
              key={file.path}
              name={file.name}
              onRemove={() => onRemoveContext(file.path)}
            />
          ))}
        </div>
      )}
      <div className="flex items-end gap-2 p-3 min-h-[--input-min-height]">
        <div className="relative shrink-0">
          <button
            className="font-mono text-caption text-text-muted py-1 px-2 rounded-sm bg-surface-overlay transition-colors duration-150 ease-out-quart whitespace-nowrap hover:bg-surface-elevated hover:text-text-secondary"
            onClick={() => setModelOpen(!modelOpen)}
            disabled={disabled && models.length > 0}
            aria-label="Select model"
          >
            {currentModel?.name || unavailableMessage}
          </button>
          {modelOpen && (
            <div className="absolute bottom-full left-0 w-60 max-h-[200px] overflow-y-auto bg-surface-elevated border border-surface-overlay rounded-md mb-1 z-[--z-dropdowns]" role="listbox">
              {models.length === 0 ? (
                <div className="flex flex-col gap-1 p-3 text-text-muted text-body-sm">
                  <strong className="text-text-primary font-semibold">{unavailableMessage}</strong>
                  {unavailableDetail && <span>{unavailableDetail}</span>}
                  {onUnavailableAction && (
                    <button className="self-start text-accent-action text-caption" type="button" onClick={onUnavailableAction}>
                      {unavailableActionLabel}
                    </button>
                  )}
                </div>
              ) : models.map((m) => {
                const isActive = m.id === selectedModel;
                const isDisabled = m.configured === false;
                const baseClass = "flex items-center justify-between w-full py-2 px-3 text-left transition-colors duration-150 ease-out-quart";
                const activeClass = isActive ? " bg-surface-overlay" : "";
                const disabledClass = isDisabled ? " opacity-40 cursor-not-allowed hover:bg-transparent" : " hover:bg-surface-overlay";
                return (
                  <button
                    key={m.id}
                    className={`${baseClass}${activeClass}${disabledClass}`}
                    role="option"
                    aria-selected={isActive}
                    disabled={isDisabled}
                    onClick={() => {
                      if (m.configured !== false) {
                        onModelChange(m.id);
                      }
                      setModelOpen(false);
                    }}
                  >
                    <span className="text-body-sm text-text-primary">{m.name}</span>
                    <span className="font-mono text-caption text-text-muted flex items-center gap-1">
                      {m.provider}
                      {m.configured === false && (
                        <span className="text-[10px] text-status-error uppercase tracking-[0.03em]">Not configured</span>
                      )}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
        <textarea
          ref={textareaRef}
          className="flex-1 min-h-[28px] max-h-[200px] resize-none font-body text-body leading-relaxed text-text-primary py-1 overflow-y-auto placeholder:text-text-muted disabled:opacity-50"
          placeholder={noModels ? unavailableMessage : "Type a prompt (Shift+Enter for new line)"}
          value={value}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          disabled={sendDisabled}
          rows={1}
          aria-label="Message input"
        />
        <button
          className="w-9 h-9 flex items-center justify-center rounded-sm bg-accent-action text-text-inverse text-body shrink-0 transition-opacity duration-150 ease-out-quart hover:opacity-90 disabled:opacity-30 disabled:cursor-not-allowed"
          disabled={sendDisabled || !value.trim()}
          onClick={() => {
            if (value.trim() && !sendDisabled) {
              onSend(value.trim());
              setValue("");
              if (textareaRef.current) {
                textareaRef.current.style.height = "auto";
              }
            }
          }}
          aria-label="Send message"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path
              d="M3 8H13M13 8L9 4M13 8L9 12"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      </div>
    </div>
  );
}
