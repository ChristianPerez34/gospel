import { useState, useRef, useCallback } from "react";
import type { ModelOption } from "../types";
import { ContextPill } from "./ContextPill";
import "./InputBar.css";

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
  const sendDisabled = disabled || models.length === 0;

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
    <div className="input-bar">
      {contextFiles.length > 0 && (
        <div className="input-bar__context">
          {contextFiles.map((file) => (
            <ContextPill
              key={file.path}
              name={file.name}
              onRemove={() => onRemoveContext(file.path)}
            />
          ))}
        </div>
      )}
      <div className="input-bar__row">
        <div className="input-bar__model-select">
          <button
            className="input-bar__model-btn"
            onClick={() => setModelOpen(!modelOpen)}
            disabled={disabled && models.length > 0}
            aria-label="Select model"
          >
            {currentModel?.name || unavailableMessage}
          </button>
          {modelOpen && (
            <div className="input-bar__model-dropdown" role="listbox">
              {models.length === 0 ? (
                <div className="input-bar__model-empty">
                  <strong>{unavailableMessage}</strong>
                  {unavailableDetail && <span>{unavailableDetail}</span>}
                  {onUnavailableAction && (
                    <button className="input-bar__model-empty-action" type="button" onClick={onUnavailableAction}>
                      {unavailableActionLabel}
                    </button>
                  )}
                </div>
              ) : models.map((m) => (
                <button
                  key={m.id}
                  className={`input-bar__model-option${
                    m.id === selectedModel ? " input-bar__model-option--active" : ""
                  }${m.configured === false ? " input-bar__model-option--disabled" : ""}`}
                  role="option"
                  aria-selected={m.id === selectedModel}
                  disabled={m.configured === false}
                  onClick={() => {
                    if (m.configured !== false) {
                      onModelChange(m.id);
                    }
                    setModelOpen(false);
                  }}
                >
                  <span className="input-bar__model-name">{m.name}</span>
                  <span className="input-bar__model-provider">
                    {m.provider}
                    {m.configured === false && (
                      <span className="input-bar__model-not-configured">Not configured</span>
                    )}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
        <textarea
          ref={textareaRef}
          className="input-bar__textarea"
          placeholder={
            sendDisabled ? unavailableMessage : "Type a prompt (Shift+Enter for new line)"
          }
          value={value}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          disabled={sendDisabled}
          rows={1}
          aria-label="Message input"
        />
        <button
          className="input-bar__send"
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
