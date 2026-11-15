import { invoke } from "@tauri-apps/api/core";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import type { ProviderConfig } from "./ProviderSelector";

interface CustomProvider {
  id: string;
  name: string;
  baseUrl: string;
  models: string[];
}

interface Draft {
  id: string | null;
  name: string;
  baseUrl: string;
  models: string;
  apiKey: string;
  removeKey: boolean;
}

const inputClass =
  "w-full min-w-0 py-1.5 px-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-body text-body-sm outline-none placeholder:text-text-muted focus:border-accent-action";

const emptyDraft = (): Draft => ({
  id: null,
  name: "",
  baseUrl: "",
  models: "",
  apiKey: "",
  removeKey: false,
});

export function CustomProvidersPanel({
  providers,
  onRefreshAvailability,
  isRefreshingModels = false,
}: {
  providers: ProviderConfig[];
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
  isRefreshingModels?: boolean;
}) {
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setCustomProviders(await invoke<CustomProvider[]>("list_custom_providers"));
    } catch (error) {
      setError(String(error));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function save() {
    if (!draft || busy || isRefreshingModels) return;
    setBusy(true);
    setError("");
    try {
      const result = await invoke<{ warning?: string | null }>("save_custom_provider", {
        id: draft.id,
        name: draft.name,
        baseUrl: draft.baseUrl,
        models: draft.models
          .split(/[\n,]/)
          .map((model) => model.trim())
          .filter(Boolean),
        apiKey: draft.apiKey || null,
        removeKey: draft.removeKey,
      });
      setDraft(null);
      if (result?.warning) setError(result.warning);
      await load();
      await onRefreshAvailability(true);
      // A simultaneous manual refresh may have started before this save and
      // caused the forced refresh to be skipped.
      await onRefreshAvailability();
    } catch (error) {
      setError(String(error));
      await load();
      await onRefreshAvailability();
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    if (busy || isRefreshingModels) return;
    setBusy(true);
    setError("");
    try {
      await invoke("delete_custom_provider", { id });
      setConfirmDelete(null);
      setDraft(null);
      await load();
      await onRefreshAvailability();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function setVisible(provider: ProviderConfig) {
    if (busy || isRefreshingModels) return;
    setBusy(true);
    setError("");
    try {
      await invoke("set_provider_visibility", {
        provider: provider.id,
        visible: !provider.visible,
      });
      await onRefreshAvailability();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      className="grid gap-3 pt-3 border-t border-surface-overlay"
      aria-label="Custom providers"
    >
      <div className="flex items-center justify-between gap-3">
        <h3 className="m-0 text-body-sm font-semibold text-text-secondary">Custom providers</h3>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            setDraft(emptyDraft());
            setError("");
          }}
        >
          <Plus aria-hidden="true" /> Add provider
        </Button>
      </div>
      {error && (
        <p role="alert" className="m-0 text-body-sm text-status-error break-words">
          {error}
        </p>
      )}
      {draft && (
        <form
          className="grid gap-3 border border-surface-overlay rounded-md p-3"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <h4 className="m-0 text-body-sm font-medium text-text-primary">
            {draft.id ? "Edit provider" : "Add provider"}
          </h4>
          <label className="grid gap-1 text-body-sm text-text-secondary">
            Name
            <input
              className={inputClass}
              required
              maxLength={100}
              value={draft.name}
              onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            />
          </label>
          <label className="grid gap-1 text-body-sm text-text-secondary">
            Base URL
            <input
              className={inputClass}
              type="url"
              required
              placeholder="https://api.example.com/v1"
              value={draft.baseUrl}
              disabled={draft.id !== null}
              onChange={(event) => setDraft({ ...draft, baseUrl: event.target.value })}
            />
          </label>
          {draft.baseUrl.trim().toLowerCase().startsWith("http://") && (
            <p className="m-0 text-caption text-status-warning">
              HTTP sends prompts and API keys without encryption.
            </p>
          )}
          <label className="grid gap-1 text-body-sm text-text-secondary">
            Model IDs (optional)
            <textarea
              className={inputClass}
              rows={2}
              placeholder="Leave empty to discover models"
              value={draft.models}
              onChange={(event) => setDraft({ ...draft, models: event.target.value })}
            />
          </label>
          <label className="grid gap-1 text-body-sm text-text-secondary">
            API key (optional)
            <input
              className={inputClass}
              type="password"
              autoComplete="off"
              disabled={draft.removeKey}
              value={draft.apiKey}
              onChange={(event) => setDraft({ ...draft, apiKey: event.target.value })}
            />
          </label>
          {draft.id && (
            <label className="flex items-center gap-2 text-body-sm text-text-secondary">
              <input
                type="checkbox"
                checked={draft.removeKey}
                onChange={(event) =>
                  setDraft({ ...draft, removeKey: event.target.checked, apiKey: "" })
                }
              />
              Remove saved API key
            </label>
          )}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => setDraft(null)}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={busy || isRefreshingModels || !draft.name.trim() || !draft.baseUrl.trim()}
            >
              Save provider
            </Button>
          </div>
        </form>
      )}
      {customProviders.map((custom) => {
        const availability = providers.find((provider) => provider.id === custom.id);
        const detail = !availability
          ? "Checking"
          : !availability.visible
            ? "Hidden"
            : availability.modelFetchStatus === "failed"
              ? availability.errorDetail || "Model load failed"
              : `${availability.modelCount} models`;
        return (
          <div
            key={custom.id}
            className="border border-surface-overlay rounded-md px-3 py-3 grid gap-2"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-baseline gap-x-2">
                  <span className="text-body-sm font-medium text-text-primary break-words">
                    {custom.name}
                  </span>
                  <span className="text-caption text-text-muted">{detail}</span>
                </div>
                <div className="text-caption text-text-muted break-all">{custom.baseUrl}</div>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={`Edit ${custom.name}`}
                  title={`Edit ${custom.name}`}
                  onClick={() => {
                    setDraft({
                      id: custom.id,
                      name: custom.name,
                      baseUrl: custom.baseUrl,
                      models: custom.models.join("\n"),
                      apiKey: "",
                      removeKey: false,
                    });
                    setError("");
                  }}
                >
                  <Pencil size={14} aria-hidden="true" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={`Delete ${custom.name}`}
                  title={`Delete ${custom.name}`}
                  onClick={() => setConfirmDelete(custom.id)}
                >
                  <Trash2 size={14} aria-hidden="true" />
                </Button>
                <button
                  type="button"
                  className={`hit-target relative w-9 h-5 border-none rounded-full p-0 shrink-0 ${availability?.visible ? "bg-accent-action" : "bg-surface-overlay"}`}
                  aria-label={`${availability?.visible ? "Hide" : "Show"} ${custom.name} ${availability?.visible ? "from" : "in"} model picker`}
                  aria-pressed={availability?.visible ?? false}
                  disabled={!availability || busy || isRefreshingModels}
                  onClick={() => availability && void setVisible(availability)}
                >
                  <span
                    className={`absolute top-0.5 left-0.5 w-4 h-4 bg-text-inverse rounded-full ${availability?.visible ? "translate-x-4" : ""}`}
                  />
                </button>
              </div>
            </div>
            {confirmDelete === custom.id && (
              <div className="flex flex-wrap items-center justify-between gap-2 text-body-sm text-status-error">
                <span>Remove {custom.name} and its saved API key?</span>
                <div className="flex gap-2">
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={busy}
                    onClick={() => setConfirmDelete(null)}
                  >
                    Cancel
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={busy || isRefreshingModels}
                    onClick={() => void remove(custom.id)}
                  >
                    Remove provider
                  </Button>
                </div>
              </div>
            )}
          </div>
        );
      })}
    </section>
  );
}
