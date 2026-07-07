import { useState, useRef, useCallback, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { ChevronDown, Cpu, GitBranch, Send } from "lucide-react";
import type { ModelOption } from "../types";
import { SlashCommandMenu } from "./SlashCommandMenu";
import { useSkills } from "../hooks/useSkills";
import { levenshtein } from "../utils/levenshtein";

interface InputBarProps {
  models: ModelOption[];
  selectedModel: string;
  selectedVariant?: string | null;
  onModelChange: (modelId: string) => void;
  onVariantChange: (variant: string | null) => void;
  onSend: (message: string, invokedSkill?: { name: string; args?: string }) => void;
  disabled?: boolean;
  unavailableMessage?: string;
  unavailableDetail?: string;
  unavailableActionLabel?: string;
  onUnavailableAction?: () => void;
  workspacePath?: string;
}

const SLASH_REGEX = /^\/([a-zA-Z0-9-]+)(?:[ \t]+([\s\S]*))?$/;

export function InputBar({
  models,
  selectedModel,
  selectedVariant = null,
  onModelChange,
  onVariantChange,
  onSend,
  disabled = false,
  unavailableMessage = "No available models",
  unavailableDetail,
  unavailableActionLabel = "Open Settings",
  onUnavailableAction,
  workspacePath,
}: InputBarProps) {
  const [value, setValue] = useState("");
  const [modelOpen, setModelOpen] = useState(false);
  const [variantOpen, setVariantOpen] = useState(false);
  const [slashFilter, setSlashFilter] = useState("");
  const [showSlashMenu, setShowSlashMenu] = useState(false);
  const [unknownSkill, setUnknownSkill] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const selectedFromMenu = useRef(false);

  const { skills, reloadSkills } = useSkills(workspacePath);

  const currentModel = models.find((m) => m.id === selectedModel);
  const variants = currentModel?.variants?.filter((variant) => !variant.deprecated) ?? [];
  const currentVariant = currentModel?.variants?.find((variant) => variant.id === selectedVariant);
  const selectedModelFallbackLabel = selectedModel
    ? selectedModel.split("::").slice(1).join(" · ")
    : "";
  const noModels = models.length === 0;
  const sendDisabled = disabled || noModels;
  const variantDisabled = disabled || currentModel?.configured === false;
  const currentModelConfigured = currentModel?.configured !== false;

  useEffect(() => {
    if (selectedFromMenu.current) {
      selectedFromMenu.current = false;
      return;
    }

    const firstLine = value.split("\n")[0] ?? "";
    if (firstLine.startsWith("/")) {
      const match = firstLine.match(/^\/([a-zA-Z0-9-]*)/);
      if (match) {
        setSlashFilter(match[1]);
        setShowSlashMenu(true);
        setUnknownSkill(null);
      } else {
        setShowSlashMenu(false);
      }
    } else {
      setShowSlashMenu(false);
      setSlashFilter("");
      setUnknownSkill(null);
    }
  }, [value]);

  useEffect(() => {
    if (!modelOpen && !variantOpen) return;
    const closeMenus = () => {
      setModelOpen(false);
      setVariantOpen(false);
    };
    const handlePointerDown = (event: MouseEvent) => {
      if (pickerRef.current?.contains(event.target as Node)) return;
      closeMenus();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMenus();
    };
    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [modelOpen, variantOpen]);

  const handleSlashSelect = useCallback(
    (name: string) => {
      selectedFromMenu.current = true;
      setValue((current) => {
        const lines = current.split("\n");
        const firstLine = lines[0] ?? "";
        const updatedFirstLine = firstLine.replace(/^\/[a-zA-Z0-9-]*/, `/${name}`);
        return [updatedFirstLine, ...lines.slice(1)].join("\n");
      });
      setShowSlashMenu(false);
      setSlashFilter("");
      setUnknownSkill(null);
      textareaRef.current?.focus();
    },
    []
  );

  const doSend = useCallback(
    (text: string) => {
      const firstLine = text.split("\n")[0] ?? text;
      const rest = text.split("\n").slice(1).join("\n").trim();
      const match = firstLine.match(SLASH_REGEX);
      if (match) {
        let skillName = match[1];
        const args = match[2]?.trim();
        const knownSkill = skills.find((s) => s.name.toLowerCase() === skillName.toLowerCase());
        if (knownSkill) {
          const promptMessage = [args, rest].filter(Boolean).join("\n");
          onSend(promptMessage || "", { name: knownSkill.name, args: args || undefined });
        } else {
          setUnknownSkill(skillName);
          return;
        }
      } else {
        onSend(text);
      }
      setValue("");
      setUnknownSkill(null);
      if (textareaRef.current) {
        textareaRef.current.style.height = "auto";
      }
    },
    [skills, onSend]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Tab" && showSlashMenu) {
        const firstLine = value.split("\n")[0] ?? "";
        const match = firstLine.match(/^\/([a-zA-Z0-9-]*)/);
        if (match) {
          const lower = match[1].toLowerCase();
          const exact = skills.find((s) => s.name.toLowerCase() === lower);
          if (!exact) {
            let bestName = "";
            let bestDist = Infinity;
            for (const s of skills) {
              const dist = levenshtein(lower, s.name.toLowerCase());
              if (dist < bestDist) {
                bestDist = dist;
                bestName = s.name;
              }
            }
            if (bestName && bestDist <= 3) {
              e.preventDefault();
              handleSlashSelect(bestName);
              return;
            }
          }
        }
      }

      if (e.key === "Escape" && unknownSkill) {
        e.preventDefault();
        onSend(value);
        setValue("");
        setUnknownSkill(null);
        if (textareaRef.current) {
          textareaRef.current.style.height = "auto";
        }
        return;
      }

      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        if (value.trim() && !sendDisabled) {
          doSend(value.trim());
        }
      }
    },
    [value, sendDisabled, doSend, showSlashMenu, skills, handleSlashSelect, unknownSkill, onSend]
  );

  const handleInput = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setValue(e.target.value);
    const el = e.target;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 200) + "px";
  }, []);

  return (
    <div className="input-bar bg-surface-elevated border-t border-surface-overlay flex shrink-0 flex-col gap-2 p-3 z-[--z-sticky-input]">
      <div className="composer-shell flex w-full max-w-[1040px] flex-col gap-2 self-center">
        <div className="relative">
          <SlashCommandMenu
            skills={skills}
            filter={slashFilter}
            visible={showSlashMenu}
            onSelect={handleSlashSelect}
            onReload={reloadSkills}
          />
        </div>
        {unknownSkill && (
          <div className="px-3 text-caption text-status-error">
            Unknown skill:{" "}
            <span className="font-mono">/{unknownSkill}</span>. Press Esc to
            send anyway.
          </div>
        )}
        <div className="composer-deck min-h-[--input-min-height] rounded-lg p-2">
          <div className="composer-control-strip flex flex-col gap-2 border-b border-surface-overlay/80 pb-2 md:flex-row md:items-center md:justify-between">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2" ref={pickerRef}>
              <div className="relative min-w-0 flex-[1_1_18rem]">
                <Button
                  variant="outline"
                  size="default"
                  className="composer-control-button h-10 w-full min-w-0 justify-start gap-2 rounded-md px-2.5"
                  onClick={() => {
                    setModelOpen((open) => !open);
                    setVariantOpen(false);
                  }}
                  disabled={disabled && models.length > 0}
                  aria-expanded={modelOpen}
                  aria-label="Select model"
                >
                  <Cpu className="size-3.5 shrink-0 text-text-muted" aria-hidden="true" />
                  <span className="flex min-w-0 flex-1 flex-col items-start leading-tight">
                    <span className="text-[10px] font-medium uppercase tracking-[0.12em] text-text-muted">
                      Model
                    </span>
                    <span className="max-w-full truncate font-mono text-caption text-text-primary">
                      {currentModel?.name || selectedModelFallbackLabel || unavailableMessage}
                    </span>
                  </span>
                  <ChevronDown className="size-3.5 shrink-0 text-text-muted" aria-hidden="true" />
                </Button>
                {modelOpen && (
                  <div className="model-menu absolute bottom-full left-0 mb-2 max-h-[240px] w-80 max-w-[calc(100vw-2rem)] overflow-y-auto rounded-lg border border-surface-overlay bg-surface-elevated z-(--z-dropdowns)" role="listbox">
                    {models.length === 0 ? (
                      <div className="flex flex-col gap-1 p-3 text-body-sm text-text-muted">
                        <strong className="font-medium text-text-primary">{unavailableMessage}</strong>
                        {unavailableDetail && <span>{unavailableDetail}</span>}
                        {onUnavailableAction && (
                          <button className="hit-target self-start text-caption text-accent-action" type="button" onClick={onUnavailableAction}>
                            {unavailableActionLabel}
                          </button>
                        )}
                      </div>
                    ) : models.map((m) => {
                      const isActive = m.id === selectedModel;
                      const isDisabled = m.configured === false;
                      const baseClass = "flex min-h-11 w-full items-center justify-between gap-3 px-3 text-left transition-colors duration-150 ease-out-quart";
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
                          <span className="truncate text-body-sm text-text-primary">{m.name}</span>
                          <span className="flex shrink-0 items-center gap-1 font-mono text-caption text-text-muted">
                            {m.provider}
                            {m.configured === false && (
                              <span className="text-caption uppercase tracking-[0.03em] text-status-error">Not configured</span>
                            )}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
              {variants.length > 0 && (
                <div className="relative min-w-[10rem] flex-1 sm:flex-none">
                  <Button
                    variant="outline"
                    size="default"
                    className="composer-control-button h-10 w-full justify-start gap-2 rounded-md px-2.5 sm:w-48"
                    onClick={() => {
                      setVariantOpen((open) => !open);
                      setModelOpen(false);
                    }}
                    disabled={variantDisabled}
                    aria-expanded={variantOpen}
                    aria-label="Select variant"
                  >
                    <GitBranch className="size-3.5 shrink-0 text-text-muted" aria-hidden="true" />
                    <span className="flex min-w-0 flex-1 flex-col items-start leading-tight">
                      <span className="text-[10px] font-medium uppercase tracking-[0.12em] text-text-muted">
                        Variant
                      </span>
                      <span className="max-w-full truncate font-mono text-caption text-text-primary">
                        {currentVariant?.name || "Default"}
                      </span>
                    </span>
                    <ChevronDown className="size-3.5 shrink-0 text-text-muted" aria-hidden="true" />
                  </Button>
                  {variantOpen && (
                    <div className="model-menu variant-menu absolute bottom-full right-0 mb-2 overflow-y-auto rounded-lg border border-surface-overlay bg-surface-elevated z-(--z-dropdowns)" role="listbox">
                      {[
                        {
                          id: null,
                          name: "Default",
                          description: `Provider default${currentModel?.provider ? ` · ${currentModel.provider}` : ""}`,
                        },
                        ...variants,
                      ].map((variant) => {
                        const isActive = selectedVariant === variant.id;
                        const isDisabled = currentModel?.configured === false;
                        const baseClass = "variant-option flex w-full flex-col items-start gap-1 px-3 py-2.5 text-left transition-colors duration-150 ease-out-quart";
                        const activeClass = isActive ? " bg-surface-overlay" : "";
                        const disabledClass = isDisabled ? " opacity-40 cursor-not-allowed hover:bg-transparent" : " hover:bg-surface-overlay";
                        return (
                          <button
                            type="button"
                            key={variant.id ?? "default"}
                            className={`${baseClass}${activeClass}${disabledClass}`}
                            role="option"
                            aria-selected={isActive}
                            disabled={isDisabled}
                            onClick={() => {
                              if (currentModel?.configured !== false) {
                                onVariantChange(variant.id);
                              }
                              setVariantOpen(false);
                            }}
                          >
                            <span className="flex w-full min-w-0 items-center justify-between gap-3">
                              <span className="truncate text-body-sm font-medium text-text-primary">{variant.name}</span>
                              {currentModel?.configured === false && (
                                <span className="text-caption uppercase tracking-[0.03em] text-status-error">Not configured</span>
                              )}
                            </span>
                            <span className="max-w-[34ch] text-body-sm leading-snug text-text-muted">
                              {variant.description}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>
            {currentModel && (
              <div className="hidden min-w-0 items-center gap-2 text-caption text-text-muted md:flex">
                <span
                  className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                    currentModelConfigured ? "bg-status-success" : "bg-status-warning"
                  }`}
                  aria-hidden="true"
                />
                <span className="truncate font-mono">
                  {currentModelConfigured ? currentModel.provider : "Not configured"}
                </span>
              </div>
            )}
          </div>
          <div className="flex items-end gap-2 pt-2">
            <textarea
              ref={textareaRef}
              className="composer-input min-h-12 max-h-[200px] flex-1 resize-none overflow-y-auto rounded-md bg-transparent px-2 py-2 font-body text-body leading-relaxed text-text-primary placeholder:text-text-muted disabled:opacity-50"
              placeholder={noModels ? unavailableMessage : "Ask Gospel to work in this workspace"}
              value={value}
              onChange={handleInput}
              onKeyDown={handleKeyDown}
              disabled={sendDisabled}
              rows={1}
              aria-label="Message input"
            />
            <Button
              variant="default"
              size="icon"
              className="size-10 rounded-md bg-accent-action text-text-inverse hover:opacity-90 disabled:opacity-30"
              disabled={sendDisabled || !value.trim()}
              onClick={() => {
                if (value.trim() && !sendDisabled) {
                  doSend(value.trim());
                }
              }}
              aria-label="Send message"
            >
              <Send className="size-4" aria-hidden="true" />
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
