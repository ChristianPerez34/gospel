import { useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  Edit3,
  Plus,
  RefreshCw,
  Save,
  ShieldCheck,
  ShieldOff,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useMcpServers } from "../hooks/useMcpServers";
import type {
  CreateMcpServerRequest,
  McpEnvValue,
  McpSafetyClass,
  McpServer,
} from "../types";

interface EnvDraft {
  id: string;
  key: string;
  value: string;
}

interface SecretDraft {
  id: string;
  key: string;
}

interface ServerDraft {
  displayName: string;
  command: string;
  argsText: string;
  safetyClass: McpSafetyClass;
  env: EnvDraft[];
  secretEnv: SecretDraft[];
  showAdvanced: boolean;
}

const inputClass =
  "w-full py-1.5 px-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-body text-body-sm outline-none transition-colors duration-150 ease-out-quart placeholder:text-text-muted focus:border-accent-action";
const monoInputClass =
  "w-full py-1.5 px-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-mono text-mono outline-none transition-colors duration-150 ease-out-quart placeholder:text-text-muted focus:border-accent-action";
const selectClass =
  "w-full py-1.5 px-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-body text-body-sm outline-none focus:border-accent-action";

function rowId() {
  return Math.random().toString(36).slice(2);
}

function emptyDraft(): ServerDraft {
  return {
    displayName: "",
    command: "",
    argsText: "",
    safetyClass: "unknown",
    env: [],
    secretEnv: [],
    showAdvanced: false,
  };
}

function draftFromServer(server: McpServer): ServerDraft {
  return {
    displayName: server.displayName,
    command: server.command ?? "",
    argsText: server.args.join("\n"),
    safetyClass: server.safetyClass,
    env: server.env.map((entry) => ({ id: rowId(), ...entry })),
    secretEnv: server.secretEnvKeys.map((key) => ({ id: rowId(), key })),
    showAdvanced: server.env.length > 0 || server.secretEnvKeys.length > 0,
  };
}

function requestFromDraft(draft: ServerDraft): CreateMcpServerRequest {
  return {
    displayName: draft.displayName.trim(),
    command: draft.command.trim(),
    args: draft.argsText
      .split("\n")
      .map((arg) => arg.trim())
      .filter(Boolean),
    env: draft.env
      .map((entry) => ({ key: entry.key.trim(), value: entry.value }))
      .filter((entry) => entry.key),
    secretEnvKeys: draft.secretEnv.map((entry) => entry.key.trim()).filter(Boolean),
    safetyClass: draft.safetyClass,
    scope: "main_and_exploration",
  };
}

function validationErrors(draft: ServerDraft) {
  const request = requestFromDraft(draft);
  const errors: string[] = [];
  if (!request.displayName) errors.push("Name is required.");
  if (!request.command) errors.push("Command is required.");

  const nonSecretKeys = request.env.map((entry) => entry.key);
  const secretKeys = request.secretEnvKeys;
  const duplicateNonSecret = firstDuplicate(nonSecretKeys);
  const duplicateSecret = firstDuplicate(secretKeys);
  const overlap = nonSecretKeys.find((key) => secretKeys.includes(key));
  if (duplicateNonSecret) errors.push(`Duplicate env key: ${duplicateNonSecret}.`);
  if (duplicateSecret) errors.push(`Duplicate secret key: ${duplicateSecret}.`);
  if (overlap) errors.push(`${overlap} cannot be both secret and non-secret.`);
  return errors;
}

function firstDuplicate(values: string[]) {
  const seen = new Set<string>();
  return values.find((value) => {
    if (seen.has(value)) return true;
    seen.add(value);
    return false;
  });
}

function labelize(value: string) {
  return value.replace(/_/g, " ");
}

function statusColor(server: McpServer) {
  if (server.health === "connected") return "text-status-success";
  if (server.health === "untrusted" || server.health === "not_found") return "text-status-error";
  return "text-text-muted";
}

export function McpSettingsPanel() {
  const {
    builtInServers,
    customServers,
    loading,
    savingId,
    error,
    importPreview,
    setImportPreview,
    setEnabled,
    trust,
    revokeTrust,
    refresh,
    create,
    update,
    remove,
    previewImport,
    applyImport,
  } = useMcpServers(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ServerDraft | null>(null);
  const [importPath, setImportPath] = useState("");
  const [selectedImports, setSelectedImports] = useState<Set<string>>(new Set());
  const [overwriteExisting, setOverwriteExisting] = useState(false);
  const [lastImportSummary, setLastImportSummary] = useState<string | null>(null);

  useEffect(() => {
    if (!importPreview) {
      setSelectedImports(new Set());
      return;
    }
    setSelectedImports(new Set(importPreview.servers.map((server) => server.externalId)));
  }, [importPreview]);

  const draftErrors = useMemo(() => (draft ? validationErrors(draft) : []), [draft]);
  const canSaveDraft = draft !== null && draftErrors.length === 0;

  async function saveDraft() {
    if (!draft || !canSaveDraft) return;
    const request = requestFromDraft(draft);
    if (editingId) {
      await update(editingId, request);
    } else {
      await create(request);
    }
    setDraft(null);
    setEditingId(null);
  }

  async function applySelectedImport() {
    if (!importPreview || selectedImports.size === 0) return;
    const result = await applyImport(
      importPreview.token,
      Array.from(selectedImports),
      overwriteExisting,
    );
    if (!result) return;
    setLastImportSummary(
      `${result.created.length} created, ${result.updated.length} updated, ${result.skipped.length} skipped`,
    );
  }

  return (
    <div className="flex flex-col gap-5">
      {error && (
        <div className="rounded-md border border-status-error bg-surface-base px-3 py-2 text-body-sm text-status-error">
          {error}
        </div>
      )}

      <section className="grid gap-3">
        <SectionHeading
          title="Built-in"
          detail={loading ? "Loading" : `${builtInServers.length} available`}
        />
        <div className="grid gap-2">
          {builtInServers.map((server) => (
            <ServerRow
              key={server.id}
              server={server}
              saving={savingId === server.id}
              onToggle={(enabled) => void setEnabled(server, enabled)}
              onRefresh={() => void refresh(server)}
            />
          ))}
        </div>
      </section>

      <section className="grid gap-3">
        <div className="flex items-center justify-between gap-3">
          <SectionHeading
            title="Custom"
            detail={`${customServers.length} configured`}
          />
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => {
              setEditingId(null);
              setDraft(emptyDraft());
            }}
          >
            <Plus aria-hidden="true" />
            Add
          </Button>
        </div>

        {draft && (
          <CustomServerEditor
            draft={draft}
            errors={draftErrors}
            editing={editingId !== null}
            saving={loading || (editingId !== null && savingId === editingId)}
            canSave={canSaveDraft}
            onChange={setDraft}
            onCancel={() => {
              setDraft(null);
              setEditingId(null);
            }}
            onSave={() => void saveDraft()}
          />
        )}

        <div className="grid gap-2">
          {customServers.length === 0 && !draft ? (
            <div className="rounded-md border border-dashed border-surface-overlay px-3 py-4 text-body-sm text-text-muted">
              No custom MCP servers configured.
            </div>
          ) : (
            customServers.map((server) => (
              <ServerRow
                key={server.id}
                server={server}
                saving={savingId === server.id}
                onToggle={(enabled) => void setEnabled(server, enabled)}
                onRefresh={() => void refresh(server)}
                onTrust={() => void trust(server)}
                onRevokeTrust={() => void revokeTrust(server)}
                onEdit={() => {
                  setEditingId(server.id);
                  setDraft(draftFromServer(server));
                }}
                onDelete={() => void remove(server)}
              />
            ))
          )}
        </div>
      </section>

      <section className="grid gap-3">
        <SectionHeading title="Import" detail="OpenCode-compatible" />
        <div className="grid gap-2">
          <div className="flex gap-2">
            <input
              className={monoInputClass}
              value={importPath}
              onChange={(event) => setImportPath(event.target.value)}
              placeholder="/path/to/opencode.json"
              aria-label="OpenCode MCP import path"
            />
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={!importPath.trim() || loading}
              onClick={() => void previewImport(importPath.trim())}
            >
              <Upload aria-hidden="true" />
              Preview
            </Button>
          </div>

          {lastImportSummary && (
            <div className="text-caption text-status-success">{lastImportSummary}</div>
          )}

          {importPreview && (
            <div className="grid gap-2 rounded-md border border-surface-overlay p-3">
              {importPreview.warnings.map((warning) => (
                <div key={warning} className="text-caption text-status-warning">
                  {warning}
                </div>
              ))}

              {importPreview.servers.map((server) => {
                const selected = selectedImports.has(server.externalId);
                return (
                  <label
                    key={server.externalId}
                    className="grid gap-2 rounded-sm border border-surface-overlay bg-surface-base p-2"
                  >
                    <span className="flex items-start justify-between gap-3">
                      <span className="flex items-center gap-2">
                        <input
                          type="checkbox"
                          checked={selected}
                          onChange={(event) => {
                            setSelectedImports((current) => {
                              const next = new Set(current);
                              if (event.target.checked) next.add(server.externalId);
                              else next.delete(server.externalId);
                              return next;
                            });
                          }}
                          aria-label={`Select ${server.name}`}
                        />
                        <span className="font-body text-body-sm font-medium text-text-primary">
                          {server.name}
                        </span>
                      </span>
                      <span className="font-mono text-caption text-text-muted">
                        {server.matchedServerId ? "match" : "new"}
                      </span>
                    </span>

                    <span className="font-mono text-caption text-text-muted">
                      {server.proposed.command} {server.proposed.args.join(" ")}
                    </span>

                    {server.fieldDiffs.length > 0 && (
                      <span className="grid gap-1">
                        {server.fieldDiffs.map((diff) => (
                          <span key={diff.field} className="text-caption text-text-muted">
                            {diff.field}: {diff.current ?? "(empty)"} -&gt; {diff.incoming ?? "(empty)"}
                          </span>
                        ))}
                      </span>
                    )}

                    {server.warnings.map((warning) => (
                      <span key={warning} className="text-caption text-status-warning">
                        {warning}
                      </span>
                    ))}
                  </label>
                );
              })}

              <label className="flex items-center gap-2 text-body-sm text-text-secondary">
                <input
                  type="checkbox"
                  checked={overwriteExisting}
                  onChange={(event) => setOverwriteExisting(event.target.checked)}
                />
                Overwrite matched local fields
              </label>

              <div className="flex items-center justify-end gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => setImportPreview(null)}
                >
                  <X aria-hidden="true" />
                  Clear
                </Button>
                <Button
                  type="button"
                  size="sm"
                  disabled={selectedImports.size === 0 || loading}
                  onClick={() => void applySelectedImport()}
                >
                  <Save aria-hidden="true" />
                  Apply
                </Button>
              </div>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}

function SectionHeading({ title, detail }: { title: string; detail: string }) {
  return (
    <div>
      <h3 className="m-0 text-heading-sm font-medium text-text-primary">{title}</h3>
      <p className="m-0 text-body-sm text-text-muted">{detail}</p>
    </div>
  );
}

function ServerRow({
  server,
  saving,
  onToggle,
  onRefresh,
  onTrust,
  onRevokeTrust,
  onEdit,
  onDelete,
}: {
  server: McpServer;
  saving: boolean;
  onToggle: (enabled: boolean) => void;
  onRefresh: () => void;
  onTrust?: () => void;
  onRevokeTrust?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  return (
    <div className="rounded-md border border-surface-overlay bg-surface-elevated p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 grid gap-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-body text-body-sm font-medium text-text-primary">
              {server.displayName}
            </span>
            <span className="rounded-sm bg-surface-overlay px-1.5 py-0.5 font-mono text-caption text-text-muted">
              {labelize(server.safetyClass)}
            </span>
            <span className={`font-mono text-caption ${statusColor(server)}`}>
              {labelize(server.health)}
            </span>
          </div>
          {server.description && (
            <div className="text-body-sm text-text-muted">{server.description}</div>
          )}
          {server.command && (
            <div className="truncate font-mono text-caption text-text-muted">
              {server.command} {server.args.join(" ")}
            </div>
          )}
          <div className="text-caption text-text-muted">
            {server.enabled ? labelize(server.readiness) : "disabled"}
            {server.inventory.length > 0 ? ` - ${server.inventory.length} tools` : ""}
          </div>
          {server.lastResolvedExecutablePath && (
            <div className="truncate font-mono text-caption text-text-muted">
              {server.lastResolvedExecutablePath}
            </div>
          )}
          {server.lastErrorSummary && (
            <div className="text-caption text-status-warning">{server.lastErrorSummary}</div>
          )}
          {server.kind === "custom" && !server.trusted && (
            <div className="text-caption text-status-error">
              Untrusted{server.trustRevokedReason ? `: ${labelize(server.trustRevokedReason)}` : ""}
            </div>
          )}
          {server.inventory.length > 0 && (
            <div className="flex flex-wrap gap-1 pt-1">
              {server.inventory.map((tool) => (
                <span
                  key={tool.name}
                  className="rounded-sm border border-surface-overlay px-1.5 py-0.5 font-mono text-caption text-text-muted"
                >
                  {tool.name}
                </span>
              ))}
            </div>
          )}
        </div>

        <div className="flex shrink-0 flex-col items-end gap-2">
          <Toggle
            checked={server.enabled}
            disabled={saving}
            label={server.enabled ? "Disable MCP server" : "Enable MCP server"}
            onChange={(checked) => onToggle(checked)}
          />
          <div className="flex flex-wrap justify-end gap-1">
            <Button
              type="button"
              size="icon-xs"
              variant="ghost"
              disabled={saving}
              onClick={onRefresh}
              title="Refresh"
              aria-label={`Refresh ${server.displayName}`}
            >
              <RefreshCw aria-hidden="true" />
            </Button>
            {server.kind === "custom" && (
              <>
                {server.trusted ? (
                  <Button
                    type="button"
                    size="icon-xs"
                    variant="ghost"
                    disabled={saving}
                    onClick={onRevokeTrust}
                    title="Revoke trust"
                    aria-label={`Revoke trust for ${server.displayName}`}
                  >
                    <ShieldOff aria-hidden="true" />
                  </Button>
                ) : (
                  <Button
                    type="button"
                    size="icon-xs"
                    variant="ghost"
                    disabled={saving}
                    onClick={onTrust}
                    title="Trust"
                    aria-label={`Trust ${server.displayName}`}
                  >
                    <ShieldCheck aria-hidden="true" />
                  </Button>
                )}
                <Button
                  type="button"
                  size="icon-xs"
                  variant="ghost"
                  disabled={saving}
                  onClick={onEdit}
                  title="Edit"
                  aria-label={`Edit ${server.displayName}`}
                >
                  <Edit3 aria-hidden="true" />
                </Button>
                <Button
                  type="button"
                  size="icon-xs"
                  variant="ghost"
                  disabled={saving}
                  onClick={onDelete}
                  title="Delete"
                  aria-label={`Delete ${server.displayName}`}
                >
                  <Trash2 aria-hidden="true" />
                </Button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function Toggle({
  checked,
  disabled,
  label,
  onChange,
}: {
  checked: boolean;
  disabled: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      className={`hit-target relative h-5 w-9 rounded-full border-none p-0 transition-colors duration-150 ease-out-quart ${
        checked ? "bg-accent-action" : "bg-surface-overlay"
      } ${disabled ? "opacity-50" : "cursor-pointer"}`}
      type="button"
      disabled={disabled}
      aria-pressed={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
    >
      <span
        className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-text-inverse transition-transform duration-150 ease-out-quart ${
          checked ? "translate-x-4" : ""
        }`}
      />
    </button>
  );
}

function CustomServerEditor({
  draft,
  errors,
  editing,
  saving,
  canSave,
  onChange,
  onCancel,
  onSave,
}: {
  draft: ServerDraft;
  errors: string[];
  editing: boolean;
  saving: boolean;
  canSave: boolean;
  onChange: (draft: ServerDraft) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const updateEnv = (id: string, patch: Partial<McpEnvValue>) => {
    onChange({
      ...draft,
      env: draft.env.map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)),
    });
  };

  const updateSecret = (id: string, key: string) => {
    onChange({
      ...draft,
      secretEnv: draft.secretEnv.map((entry) => (entry.id === id ? { ...entry, key } : entry)),
    });
  };

  return (
    <div className="grid gap-3 rounded-md border border-accent-action bg-surface-elevated p-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h4 className="m-0 text-body-sm font-medium text-text-primary">
            {editing ? "Edit custom server" : "New custom server"}
          </h4>
          <p className="m-0 text-caption text-text-muted">
            Custom servers stay disabled and untrusted until explicitly enabled and trusted.
          </p>
        </div>
        <Button type="button" size="icon-xs" variant="ghost" onClick={onCancel} aria-label="Cancel">
          <X aria-hidden="true" />
        </Button>
      </div>

      <div className="grid gap-2">
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          Name
          <input
            className={inputClass}
            value={draft.displayName}
            onChange={(event) => onChange({ ...draft, displayName: event.target.value })}
          />
        </label>
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          Command
          <input
            className={monoInputClass}
            value={draft.command}
            onChange={(event) => onChange({ ...draft, command: event.target.value })}
            placeholder="node"
          />
        </label>
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          Args
          <textarea
            className={`${monoInputClass} min-h-[68px] resize-y`}
            value={draft.argsText}
            onChange={(event) => onChange({ ...draft, argsText: event.target.value })}
            placeholder={"server.js\n--stdio"}
          />
        </label>
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          Safety
          <select
            className={selectClass}
            value={draft.safetyClass}
            onChange={(event) =>
              onChange({ ...draft, safetyClass: event.target.value as McpSafetyClass })
            }
          >
            <option value="unknown">Unknown</option>
            <option value="read_only">Read only</option>
            <option value="mutating">Mutating</option>
          </select>
        </label>
      </div>

      <button
        type="button"
        className="flex items-center gap-1 border-none bg-transparent p-0 text-left text-body-sm text-text-secondary"
        onClick={() => onChange({ ...draft, showAdvanced: !draft.showAdvanced })}
      >
        <ChevronDown
          aria-hidden="true"
          className={`size-4 transition-transform ${draft.showAdvanced ? "rotate-180" : ""}`}
        />
        Environment
      </button>

      {draft.showAdvanced && (
        <div className="grid gap-3">
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <div className="text-caption font-medium uppercase text-text-muted">Non-secret env</div>
              <Button
                type="button"
                size="xs"
                variant="outline"
                onClick={() =>
                  onChange({
                    ...draft,
                    env: [...draft.env, { id: rowId(), key: "", value: "" }],
                  })
                }
              >
                <Plus aria-hidden="true" />
                Row
              </Button>
            </div>
            {draft.env.map((entry) => (
              <div key={entry.id} className="grid grid-cols-[1fr_1fr_auto] gap-2">
                <input
                  className={monoInputClass}
                  value={entry.key}
                  onChange={(event) => updateEnv(entry.id, { key: event.target.value })}
                  placeholder="KEY"
                />
                <input
                  className={monoInputClass}
                  value={entry.value}
                  onChange={(event) => updateEnv(entry.id, { value: event.target.value })}
                  placeholder="value"
                />
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  onClick={() =>
                    onChange({ ...draft, env: draft.env.filter((item) => item.id !== entry.id) })
                  }
                  aria-label="Remove env row"
                >
                  <X aria-hidden="true" />
                </Button>
              </div>
            ))}
          </div>

          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <div className="text-caption font-medium uppercase text-text-muted">Required secrets</div>
              <Button
                type="button"
                size="xs"
                variant="outline"
                onClick={() =>
                  onChange({
                    ...draft,
                    secretEnv: [...draft.secretEnv, { id: rowId(), key: "" }],
                  })
                }
              >
                <Plus aria-hidden="true" />
                Key
              </Button>
            </div>
            {draft.secretEnv.map((entry) => (
              <div key={entry.id} className="grid grid-cols-[1fr_auto] gap-2">
                <input
                  className={monoInputClass}
                  value={entry.key}
                  onChange={(event) => updateSecret(entry.id, event.target.value)}
                  placeholder="API_KEY"
                />
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  onClick={() =>
                    onChange({
                      ...draft,
                      secretEnv: draft.secretEnv.filter((item) => item.id !== entry.id),
                    })
                  }
                  aria-label="Remove secret env row"
                >
                  <X aria-hidden="true" />
                </Button>
              </div>
            ))}
          </div>
        </div>
      )}

      {errors.length > 0 && (
        <div className="grid gap-1 rounded-sm border border-status-error bg-surface-base px-2 py-1.5">
          {errors.map((error) => (
            <div key={error} className="text-caption text-status-error">
              {error}
            </div>
          ))}
        </div>
      )}

      <div className="flex justify-end gap-2">
        <Button type="button" size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="button" size="sm" disabled={!canSave || saving} onClick={onSave}>
          <Save aria-hidden="true" />
          {saving ? "Saving" : "Save"}
        </Button>
      </div>
    </div>
  );
}
