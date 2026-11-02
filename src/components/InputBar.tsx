import { ChevronDown, Cpu, Eye, EyeOff, GitBranch, Search, Send, Square, Star } from "lucide-react";
import {
  type CSSProperties,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Button } from "@/components/ui/button";
import type { ModelPreferences } from "../hooks/useModelPreferences";
import { useSkills } from "../hooks/useSkills";
import type { ModelOption } from "../types";
import { filterSkills } from "../utils/skillSearch";
import { SlashCommandMenu } from "./SlashCommandMenu";

interface InputBarProps {
  models: ModelOption[];
  selectedModel: string;
  selectedVariant?: string | null;
  modelPreferences: ModelPreferences;
  onModelChange: (modelId: string) => void;
  onVariantChange: (variant: string | null) => void;
  onSend: (message: string, invokedSkill?: { name: string; args?: string }) => void;
  onCancelStream?: () => void;
  isStreaming?: boolean;
  disabled?: boolean;
  unavailableMessage?: string;
  unavailableDetail?: string;
  unavailableActionLabel?: string;
  onUnavailableAction?: () => void;
  workspacePath?: string;
}

const SLASH_REGEX = /^\/([a-zA-Z0-9-]+)(?:[ \t]+([\s\S]*))?$/;
const VARIANT_MENU_MAX_WIDTH = 352;
const VARIANT_MENU_MAX_HEIGHT = 288;
const MENU_GAP = 8;
const MENU_EDGE_INSET = 8;
const MENU_MIN_USEFUL_HEIGHT = 160;

type VariantMenuPlacement = "above" | "below";

interface VariantMenuLayout {
  left: number;
  width: number;
  maxHeight: number;
  placement: VariantMenuPlacement;
}

function clipsOverflow(value: string) {
  return value
    .split(" ")
    .some((part) => part === "auto" || part === "clip" || part === "hidden" || part === "scroll");
}

function getVisibleBounds(anchor: HTMLElement) {
  const viewport = window.visualViewport;
  const viewportLeft = viewport?.offsetLeft ?? 0;
  const viewportTop = viewport?.offsetTop ?? 0;
  const bounds = {
    left: viewportLeft,
    right: viewportLeft + (viewport?.width ?? window.innerWidth),
    top: viewportTop,
    bottom: viewportTop + (viewport?.height ?? window.innerHeight),
  };

  let ancestor = anchor.parentElement;
  while (ancestor) {
    const style = window.getComputedStyle(ancestor);
    const rect = ancestor.getBoundingClientRect();
    const clipsX = clipsOverflow(style.overflowX) || clipsOverflow(style.overflow);
    const clipsY = clipsOverflow(style.overflowY) || clipsOverflow(style.overflow);

    if (clipsX) {
      bounds.left = Math.max(bounds.left, rect.left);
      bounds.right = Math.min(bounds.right, rect.right);
    }
    if (clipsY) {
      bounds.top = Math.max(bounds.top, rect.top);
      bounds.bottom = Math.min(bounds.bottom, rect.bottom);
    }

    ancestor = ancestor.parentElement;
  }

  return bounds;
}

function getVariantMenuLayout(anchor: HTMLElement): VariantMenuLayout {
  const anchorRect = anchor.getBoundingClientRect();
  const bounds = getVisibleBounds(anchor);
  const availableWidth = Math.max(0, bounds.right - bounds.left - MENU_EDGE_INSET * 2);
  const width = Math.min(VARIANT_MENU_MAX_WIDTH, availableWidth);
  const minimumLeft = bounds.left + MENU_EDGE_INSET;
  const maximumLeft = Math.max(minimumLeft, bounds.right - MENU_EDGE_INSET - width);
  const viewportLeft = Math.min(Math.max(anchorRect.right - width, minimumLeft), maximumLeft);
  const spaceAbove = Math.max(0, anchorRect.top - MENU_GAP - (bounds.top + MENU_EDGE_INSET));
  const spaceBelow = Math.max(0, bounds.bottom - MENU_EDGE_INSET - anchorRect.bottom - MENU_GAP);
  const placement: VariantMenuPlacement =
    spaceAbove >= Math.min(MENU_MIN_USEFUL_HEIGHT, VARIANT_MENU_MAX_HEIGHT) ||
    spaceAbove >= spaceBelow
      ? "above"
      : "below";

  return {
    left: viewportLeft - anchorRect.left,
    width,
    maxHeight: Math.min(VARIANT_MENU_MAX_HEIGHT, placement === "above" ? spaceAbove : spaceBelow),
    placement,
  };
}

export function InputBar({
  models,
  selectedModel,
  selectedVariant = null,
  modelPreferences,
  onModelChange,
  onVariantChange,
  onSend,
  onCancelStream,
  isStreaming = false,
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
  const [variantMenuLayout, setVariantMenuLayout] = useState<VariantMenuLayout | null>(null);
  const [modelQuery, setModelQuery] = useState("");
  const [showHiddenModels, setShowHiddenModels] = useState(false);
  const [activeOptionIndex, setActiveOptionIndex] = useState(0);
  const [slashFilter, setSlashFilter] = useState("");
  const [showSlashMenu, setShowSlashMenu] = useState(false);
  const [unknownSkill, setUnknownSkill] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const variantPickerRef = useRef<HTMLDivElement>(null);
  const modelTriggerRef = useRef<HTMLButtonElement>(null);
  const modelSearchRef = useRef<HTMLInputElement>(null);
  const modelOptionRefs = useRef<Array<HTMLDivElement | null>>([]);
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

  const { visibleModels, hiddenModels } = useMemo(() => {
    const tokens = modelQuery.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const matches = (option: ModelOption) => {
      if (tokens.length === 0) return true;
      const haystack =
        `${option.name} ${option.provider} ${option.providerLabel ?? ""} ${option.model}`.toLowerCase();
      return tokens.every((token) => haystack.includes(token));
    };
    const visible: ModelOption[] = [];
    const hiddenList: ModelOption[] = [];
    for (const option of models) {
      if (!matches(option)) continue;
      (modelPreferences.hidden.has(option.id) ? hiddenList : visible).push(option);
    }
    visible.sort(
      (a, b) =>
        Number(modelPreferences.favorites.has(b.id)) - Number(modelPreferences.favorites.has(a.id))
    );
    return { visibleModels: visible, hiddenModels: hiddenList };
  }, [models, modelQuery, modelPreferences]);

  const activeOptionIndexClamped = Math.min(
    activeOptionIndex,
    Math.max(visibleModels.length - 1, 0)
  );

  const selectModelFromMenu = useCallback(
    (option: ModelOption) => {
      if (option.configured === false) return;
      onModelChange(option.id);
      setModelOpen(false);
      modelTriggerRef.current?.focus();
    },
    [onModelChange]
  );

  const stepActiveOption = useCallback(
    (direction: 1 | -1) => {
      setActiveOptionIndex((index) => {
        let next = Math.min(index, Math.max(visibleModels.length - 1, 0));
        for (let steps = 0; steps < visibleModels.length; steps += 1) {
          next += direction;
          if (next < 0 || next >= visibleModels.length) break;
          if (visibleModels[next].configured !== false) return next;
        }
        return index;
      });
    },
    [visibleModels]
  );

  const handleModelSearchKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        stepActiveOption(1);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        stepActiveOption(-1);
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        const target = visibleModels[activeOptionIndexClamped];
        if (target) selectModelFromMenu(target);
      }
    },
    [visibleModels, activeOptionIndexClamped, selectModelFromMenu, stepActiveOption]
  );

  useEffect(() => {
    if (selectedFromMenu.current) {
      selectedFromMenu.current = false;
      return;
    }

    const firstLine = value.split("\n")[0] ?? "";
    if (firstLine.startsWith("/")) {
      const hasSpace = /\s/.test(firstLine);
      const match = firstLine.match(/^\/([a-zA-Z0-9-]*)/);
      if (match && !hasSpace) {
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
      if (event.key === "Escape") {
        closeMenus();
        if (modelOpen) modelTriggerRef.current?.focus();
      }
    };
    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [modelOpen, variantOpen]);

  useEffect(() => {
    if (!modelOpen) return;
    setModelQuery("");
    setShowHiddenModels(false);
    setActiveOptionIndex(0);
    modelSearchRef.current?.focus();
  }, [modelOpen]);

  useEffect(() => {
    if (!modelOpen) return;
    modelOptionRefs.current[activeOptionIndexClamped]?.scrollIntoView?.({ block: "nearest" });
  }, [modelOpen, activeOptionIndexClamped]);

  useLayoutEffect(() => {
    if (!variantOpen || !variantPickerRef.current) return;

    const anchor = variantPickerRef.current;
    const updateLayout = () => setVariantMenuLayout(getVariantMenuLayout(anchor));
    updateLayout();

    const resizeObserver =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(updateLayout);
    resizeObserver?.observe(anchor);
    if (pickerRef.current) resizeObserver?.observe(pickerRef.current);

    window.addEventListener("resize", updateLayout);
    window.addEventListener("scroll", updateLayout, true);
    window.visualViewport?.addEventListener("resize", updateLayout);
    window.visualViewport?.addEventListener("scroll", updateLayout);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("resize", updateLayout);
      window.removeEventListener("scroll", updateLayout, true);
      window.visualViewport?.removeEventListener("resize", updateLayout);
      window.visualViewport?.removeEventListener("scroll", updateLayout);
    };
  }, [variantOpen]);

  const variantMenuStyle: CSSProperties | undefined = variantMenuLayout
    ? {
        left: variantMenuLayout.left,
        width: variantMenuLayout.width,
        maxHeight: variantMenuLayout.maxHeight,
        top: variantMenuLayout.placement === "below" ? "calc(100% + 0.5rem)" : "auto",
        bottom: variantMenuLayout.placement === "above" ? "calc(100% + 0.5rem)" : "auto",
        transformOrigin: variantMenuLayout.placement === "above" ? "bottom left" : "top left",
      }
    : undefined;

  const handleSlashSelect = useCallback((name: string) => {
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
  }, []);

  const doSend = useCallback(
    (text: string) => {
      const firstLine = text.split("\n")[0] ?? text;
      const rest = text.split("\n").slice(1).join("\n").trim();
      const match = firstLine.match(SLASH_REGEX);
      if (match) {
        const skillName = match[1];
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
            const { filtered, suggestion } = filterSkills(skills, lower);
            const target = filtered[0]?.name ?? suggestion;
            if (target) {
              e.preventDefault();
              handleSlashSelect(target);
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
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
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
            Unknown skill: <span className="font-mono">/{unknownSkill}</span>. Press Esc to send
            anyway.
          </div>
        )}
        <div className="composer-deck min-h-[--input-min-height] rounded-lg p-2">
          <div className="composer-control-strip flex flex-col gap-2 border-b border-surface-overlay/80 pb-2 md:flex-row md:items-center md:justify-between">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2" ref={pickerRef}>
              <div className="relative min-w-0 flex-[1_1_18rem]">
                <Button
                  ref={modelTriggerRef}
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
                    <span
                      className="max-w-full truncate font-mono text-caption text-text-primary"
                      title={currentModel?.name || selectedModelFallbackLabel || undefined}
                    >
                      {currentModel?.name || selectedModelFallbackLabel || unavailableMessage}
                    </span>
                  </span>
                  <ChevronDown className="size-3.5 shrink-0 text-text-muted" aria-hidden="true" />
                </Button>
                {modelOpen && (
                  <div className="model-menu absolute bottom-full left-0 mb-2 flex max-h-[320px] w-96 max-w-[calc(100vw-2rem)] origin-bottom-left flex-col overflow-hidden rounded-lg border border-surface-overlay bg-surface-elevated z-(--z-dropdowns)">
                    {models.length === 0 ? (
                      <div className="flex flex-col gap-1 overflow-y-auto p-3 text-body-sm text-text-muted">
                        <strong className="font-medium text-text-primary">
                          {unavailableMessage}
                        </strong>
                        {unavailableDetail && <span>{unavailableDetail}</span>}
                        {onUnavailableAction && (
                          <button
                            className="hit-target self-start text-caption text-accent-structure"
                            type="button"
                            onClick={onUnavailableAction}
                          >
                            {unavailableActionLabel}
                          </button>
                        )}
                      </div>
                    ) : (
                      <>
                        <div className="border-b border-surface-overlay p-2">
                          <div className="relative flex items-center">
                            <Search
                              className="pointer-events-none absolute left-2.5 size-3.5 text-text-muted"
                              aria-hidden="true"
                            />
                            <input
                              ref={modelSearchRef}
                              role="combobox"
                              aria-autocomplete="list"
                              aria-expanded="true"
                              aria-controls="model-menu-listbox"
                              aria-activedescendant={
                                visibleModels.length > 0
                                  ? `model-option-${activeOptionIndexClamped}`
                                  : undefined
                              }
                              aria-label="Search models"
                              className="h-8 w-full rounded-md border border-transparent bg-surface-base pl-8 pr-2.5 font-body text-body-sm text-text-primary outline-none transition-colors duration-150 ease-out-quart placeholder:text-text-muted focus:border-accent-action"
                              placeholder="Search models"
                              value={modelQuery}
                              onChange={(event) => {
                                // Search changes intentionally reset keyboard selection.
                                setModelQuery(event.target.value);
                                setActiveOptionIndex(0);
                              }}
                              onKeyDown={handleModelSearchKeyDown}
                            />
                          </div>
                        </div>
                        <div className="flex-1 overflow-y-auto">
                          <div id="model-menu-listbox" role="listbox" aria-label="Models">
                            {visibleModels.map((m, index) => {
                              const isActive = m.id === selectedModel;
                              const isDisabled = m.configured === false;
                              const isFavorite = modelPreferences.favorites.has(m.id);
                              const isHighlighted = index === activeOptionIndexClamped;
                              return (
                                <div
                                  key={m.id}
                                  ref={(el) => {
                                    modelOptionRefs.current[index] = el;
                                  }}
                                  className={`group flex min-h-11 w-full items-center gap-0.5 px-2 transition-colors duration-150 ease-out-quart ${
                                    isHighlighted ? "bg-surface-overlay" : ""
                                  }`}
                                >
                                  <button
                                    type="button"
                                    id={`model-option-${index}`}
                                    onMouseEnter={() => setActiveOptionIndex(index)}
                                    className={`flex min-w-0 flex-1 items-center justify-between gap-3 px-1 py-2 text-left ${
                                      isDisabled
                                        ? "cursor-not-allowed opacity-40"
                                        : "cursor-pointer"
                                    }`}
                                    role="option"
                                    aria-selected={isActive}
                                    aria-disabled={isDisabled || undefined}
                                    tabIndex={-1}
                                    disabled={isDisabled}
                                    onClick={() => selectModelFromMenu(m)}
                                  >
                                    <span className="min-w-0 break-words text-body-sm leading-snug text-text-primary">
                                      {m.name}
                                    </span>
                                    <span className="flex shrink-0 items-center gap-1 font-mono text-caption text-text-muted">
                                      {m.providerLabel ?? m.provider}
                                      {m.configured === false && (
                                        <span className="text-caption uppercase tracking-[0.03em] text-status-error">
                                          Not configured
                                        </span>
                                      )}
                                    </span>
                                  </button>
                                  <button
                                    type="button"
                                    className="flex size-7 shrink-0 items-center justify-center rounded-md text-text-muted transition-[color,transform] duration-150 ease-out-quart hover:text-text-primary active:scale-90"
                                    aria-label={
                                      isFavorite
                                        ? `Remove ${m.name} from favorites`
                                        : `Add ${m.name} to favorites`
                                    }
                                    aria-pressed={isFavorite}
                                    title={
                                      isFavorite ? "Remove from favorites" : "Add to favorites"
                                    }
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      modelPreferences.onToggleFavorite(m.id);
                                    }}
                                  >
                                    <Star
                                      className={`size-3.5 ${isFavorite ? "fill-current text-accent-structure" : ""}`}
                                      aria-hidden="true"
                                    />
                                  </button>
                                  <button
                                    type="button"
                                    className="flex size-7 shrink-0 items-center justify-center rounded-md text-text-muted transition-[color,transform] duration-150 ease-out-quart hover:text-text-primary active:scale-90"
                                    aria-label={`Hide ${m.name} from picker`}
                                    title="Hide from picker"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      modelPreferences.onToggleHidden(m.id);
                                    }}
                                  >
                                    <Eye className="size-3.5" aria-hidden="true" />
                                  </button>
                                </div>
                              );
                            })}
                            {visibleModels.length === 0 && (
                              <div
                                className="px-3 py-4 text-body-sm text-text-muted"
                                role="option"
                                aria-selected={false}
                                aria-disabled="true"
                                tabIndex={-1}
                              >
                                {modelQuery.trim() ? "No matching models." : "No visible models."}
                              </div>
                            )}
                          </div>
                          {hiddenModels.length > 0 && (
                            <div className="border-t border-surface-overlay">
                              <button
                                type="button"
                                className="flex min-h-9 w-full cursor-pointer items-center justify-between px-3 text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:text-text-secondary"
                                onClick={() => setShowHiddenModels((open) => !open)}
                                aria-expanded={showHiddenModels}
                              >
                                <span>
                                  {showHiddenModels ? "Hide" : "Show"} {hiddenModels.length} hidden{" "}
                                  {hiddenModels.length === 1 ? "model" : "models"}
                                </span>
                                <ChevronDown
                                  className={`size-3.5 transition-transform duration-150 ease-out-quart ${showHiddenModels ? "rotate-180" : ""}`}
                                  aria-hidden="true"
                                />
                              </button>
                              {showHiddenModels &&
                                hiddenModels.map((m) => (
                                  <div
                                    key={m.id}
                                    className="flex min-h-9 w-full items-center gap-0.5 px-2 opacity-50"
                                  >
                                    <span className="min-w-0 flex-1 break-words px-1 text-body-sm leading-snug text-text-primary">
                                      {m.name}
                                    </span>
                                    <span className="shrink-0 font-mono text-caption text-text-muted">
                                      {m.providerLabel ?? m.provider}
                                    </span>
                                    <button
                                      type="button"
                                      className="flex size-7 shrink-0 items-center justify-center rounded-md text-text-muted transition-[color,transform] duration-150 ease-out-quart hover:text-text-primary active:scale-90"
                                      aria-label={`Show ${m.name} in picker`}
                                      title="Show in picker"
                                      onClick={() => modelPreferences.onToggleHidden(m.id)}
                                    >
                                      <EyeOff className="size-3.5" aria-hidden="true" />
                                    </button>
                                  </div>
                                ))}
                            </div>
                          )}
                        </div>
                      </>
                    )}
                  </div>
                )}
              </div>
              {variants.length > 0 && (
                <div ref={variantPickerRef} className="relative min-w-[10rem] flex-1 sm:flex-none">
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
                    <div
                      className="model-menu variant-menu absolute overflow-y-auto rounded-lg border border-surface-overlay bg-surface-elevated z-(--z-dropdowns)"
                      style={variantMenuStyle}
                      role="listbox"
                    >
                      {[
                        {
                          id: null,
                          name: "Default",
                          description: `Provider default${currentModel?.provider ? ` · ${currentModel.providerLabel ?? currentModel.provider}` : ""}`,
                        },
                        ...variants,
                      ].map((variant) => {
                        const isActive = selectedVariant === variant.id;
                        const isDisabled = currentModel?.configured === false;
                        const baseClass =
                          "variant-option flex w-full flex-col items-start gap-1 px-3 py-2.5 text-left transition-colors duration-150 ease-out-quart";
                        const activeClass = isActive ? " bg-surface-overlay" : "";
                        const disabledClass = isDisabled
                          ? " opacity-40 cursor-not-allowed hover:bg-transparent"
                          : " hover:bg-surface-overlay";
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
                              <span className="truncate text-body-sm font-medium text-text-primary">
                                {variant.name}
                              </span>
                              {currentModel?.configured === false && (
                                <span className="text-caption uppercase tracking-[0.03em] text-status-error">
                                  Not configured
                                </span>
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
                  {currentModelConfigured
                    ? (currentModel.providerLabel ?? currentModel.provider)
                    : "Not configured"}
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
            {isStreaming && onCancelStream ? (
              <Button
                variant="default"
                size="icon"
                className="size-10 rounded-md bg-status-error text-text-inverse hover:opacity-90"
                onClick={() => {
                  onCancelStream();
                }}
                aria-label="Stop streaming"
              >
                <Square className="size-4" aria-hidden="true" />
              </Button>
            ) : (
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
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
