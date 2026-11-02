import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  ChevronDown,
  Edit3,
  FolderOpen,
  KeyRound,
  Lock,
  Plus,
  RefreshCw,
  Save,
  ShieldCheck,
  ShieldOff,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { useMcpServers } from "../hooks/useMcpServers";
import {
  applySourceText,
  emptyDraft,
  isLikelySecretKey,
  type PairDraft,
  pairDrafts,
  requestFromDraft,
  rowId,
  type ServerDraft,
  suggestName,
  validationErrors,
} from "../lib/mcpSources";
import type { McpServer } from "../types";

const inputClass =
  "w-full py-1.5 px-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-body text-body-sm outline-none transition-colors duration-150 ease-out-quart placeholder:text-text-muted focus:border-accent-action";
const monoInputClass =
  "w-full py-1.5 px-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-mono text-mono outline-none transition-colors duration-150 ease-out-quart placeholder:text-text-muted focus:border-accent-action";

function draftFromServer(server: McpServer): ServerDraft {
  const commandLine =
    server.transport === "http"
      ? ""
      : [server.command ?? "", ...server.args].filter(Boolean).join(" ");
  return {
    sourceText: "",
    transport: server.transport,
    displayName: server.displayName,
    nameTouched: true,
    commandLine,
    url: server.url ?? "",
    env: pairDrafts(server.env, server.secretEnvKeys, server.missingSecretEnvKeys),
    headers: pairDrafts(server.headers, server.secretHeaderKeys, server.missingSecretHeaderKeys),
    showAdvanced:
      server.env.length +
        server.secretEnvKeys.length +
        server.headers.length +
        server.secretHeaderKeys.length >
      0,
  };
}

function labelize(value: string) {
  return value.replace(/_/g, " ");
}

function healthLabel(server: McpServer) {
  if (server.health === "auth_required") return "sign-in required";
  if (server.health === "untrusted") return "not approved";
  return labelize(server.health);
}

function statusColor(server: McpServer) {
  if (server.health === "connected") return "text-status-success";
  if (server.health === "untrusted" || server.health === "not_found") return "text-status-error";
  if (server.health === "auth_required") return "text-status-warning";
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
    addAndConnect,
    startOAuth,
    logoutOAuth,
    update,
    remove,
    previewImport,
    applyImport,
  } = useMcpServers(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ServerDraft | null>(null);

  const draftErrors = useMemo(() => (draft ? validationErrors(draft) : []), [draft]);
  const canSaveDraft = draft !== null && draftErrors.length === 0;

  async function saveDraft() {
    if (!draft || !canSaveDraft) return;
    const request = requestFromDraft(draft);
    if (editingId) {
      await update(editingId, request);
    } else {
      await addAndConnect(request);
    }
    setDraft(null);
    setEditingId(null);
  }

  return (
    <div className="flex flex-col gap-5">
      {error && (
        <div className="rounded-md border border-status-error bg-surface-base px-3 py-2 text-body-sm text-status-error">
          {error}
        </div>
      )}

      {builtInServers.length > 0 && (
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
      )}

      <section className="grid gap-3">
        <div className="flex items-center justify-between gap-3">
          <SectionHeading title="Servers" detail={`${customServers.length} configured`} />
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
              No MCP servers yet. Add a URL or command to give the agent more tools.
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
                onSignIn={() => void startOAuth(server)}
                onSignOut={() => void logoutOAuth(server)}
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

      <ImportSection
        loading={loading}
        importPreview={importPreview}
        setImportPreview={setImportPreview}
        previewImport={previewImport}
        applyImport={applyImport}
      />
    </div>
  );
}

type McpHook = ReturnType<typeof useMcpServers>;

function ImportSection({
  loading,
  importPreview,
  setImportPreview,
  previewImport,
  applyImport,
}: {
  loading: boolean;
  importPreview: McpHook["importPreview"];
  setImportPreview: McpHook["setImportPreview"];
  previewImport: McpHook["previewImport"];
  applyImport: McpHook["applyImport"];
}) {
  const [importPath, setImportPath] = useState("");
  /** `null` = "all incoming servers selected" (the default for a fresh preview). */
  const [selectedOverrides, setSelectedOverrides] = useState<Set<string> | null>(null);
  const [overwriteExisting, setOverwriteExisting] = useState(false);
  const [lastImportSummary, setLastImportSummary] = useState<string | null>(null);

  const selectedImports =
    selectedOverrides ?? new Set(importPreview?.servers.map((server) => server.externalId) ?? []);

  async function preview(path: string) {
    setSelectedOverrides(null);
    setLastImportSummary(null);
    await previewImport(path);
  }

  async function browseImport() {
    const selected = await openFileDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof selected === "string" && selected) {
      setImportPath(selected);
      await preview(selected);
    }
  }

  function toggleImport(externalId: string, checked: boolean) {
    setSelectedOverrides((current) => {
      const next = new Set(
        current ?? importPreview?.servers.map((server) => server.externalId) ?? []
      );
      if (checked) next.add(externalId);
      else next.delete(externalId);
      return next;
    });
  }

  async function applySelectedImport() {
    if (!importPreview || selectedImports.size === 0) return;
    const result = await applyImport(
      importPreview.token,
      Array.from(selectedImports),
      overwriteExisting
    );
    if (!result) return;
    setLastImportSummary(
      `${result.created.length} created, ${result.updated.length} updated, ${result.skipped.length} skipped`
    );
  }

  return (
    <section className="grid gap-3">
      <SectionHeading title="Import" detail="Claude, Cursor, OpenCode, VS Code configs" />
      <div className="grid gap-2">
        <div className="flex gap-2">
          <input
            className={monoInputClass}
            value={importPath}
            onChange={(event) => setImportPath(event.target.value)}
            placeholder="/path/to/mcp-config.json"
            aria-label="MCP config import path"
          />
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={loading}
            onClick={() => void browseImport()}
          >
            <FolderOpen aria-hidden="true" />
            Browse
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={!importPath.trim() || loading}
            onClick={() => void preview(importPath.trim())}
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
                        onChange={(event) => toggleImport(server.externalId, event.target.checked)}
                        aria-label={`Select ${server.name}`}
                      />
                      <span className="font-body text-body-sm font-medium text-text-primary">
                        {server.name}
                      </span>
                    </span>
                    <span className="font-mono text-caption text-text-muted">
                      {server.proposed.transport ?? "stdio"}
                      {server.matchedServerId ? " - match" : " - new"}
                    </span>
                  </span>

                  <span className="font-mono text-caption text-text-muted">
                    {server.proposed.url ??
                      `${server.proposed.command ?? ""} ${(server.proposed.args ?? []).join(" ")}`}
                  </span>

                  {server.fieldDiffs.length > 0 && (
                    <span className="grid gap-1">
                      {server.fieldDiffs.map((diff) => (
                        <span key={diff.field} className="text-caption text-text-muted">
                          {diff.field}: {diff.current ?? "(empty)"} -&gt;{" "}
                          {diff.incoming ?? "(empty)"}
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
                onClick={() => {
                  setImportPreview(null);
                  setSelectedOverrides(null);
                }}
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

function ServerDetails({
  server,
  saving,
  onSignIn,
  onSignOut,
}: {
  server: McpServer;
  saving: boolean;
  onSignIn?: () => void;
  onSignOut?: () => void;
}) {
  const mutatingCount = server.inventory.filter((tool) => !tool.readOnly).length;
  const missingSecrets = [...server.missingSecretEnvKeys, ...server.missingSecretHeaderKeys];
  const needsSignIn =
    server.transport === "http" &&
    server.trusted &&
    (server.authStatus === "required" || server.health === "auth_required");
  const signedIn = server.transport === "http" && server.authStatus === "authorized";

  return (
    <div className="min-w-0 grid gap-1">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-body text-body-sm font-medium text-text-primary">
          {server.displayName}
        </span>
        <span className="rounded-sm bg-surface-overlay px-1.5 py-0.5 font-mono text-caption text-text-muted">
          {server.transport}
        </span>
        <span className={`font-mono text-caption ${statusColor(server)}`}>
          {server.enabled ? healthLabel(server) : "disabled"}
        </span>
      </div>
      {server.description && (
        <div className="text-body-sm text-text-muted">{server.description}</div>
      )}
      <div className="truncate font-mono text-caption text-text-muted">
        {server.transport === "http"
          ? server.url
          : `${server.command ?? ""} ${server.args.join(" ")}`.trim()}
      </div>
      {server.inventory.length > 0 && (
        <div className="text-caption text-text-muted">
          {server.inventory.length} tools
          {mutatingCount > 0 ? ` (${mutatingCount} need approval)` : ""}
        </div>
      )}
      {server.lastResolvedExecutablePath && (
        <div className="truncate font-mono text-caption text-text-muted">
          {server.lastResolvedExecutablePath}
        </div>
      )}
      <ServerNotices server={server} missingSecrets={missingSecrets} />
      <OAuthControls
        saving={saving}
        needsSignIn={needsSignIn}
        signedIn={signedIn}
        onSignIn={onSignIn}
        onSignOut={onSignOut}
      />
      {server.inventory.length > 0 && <ToolChips inventory={server.inventory} />}
    </div>
  );
}

function ServerNotices({
  server,
  missingSecrets,
}: {
  server: McpServer;
  missingSecrets: string[];
}) {
  return (
    <>
      {server.lastErrorSummary && (
        <div className="text-caption text-status-warning">{server.lastErrorSummary}</div>
      )}
      {server.kind === "custom" && !server.trusted && (
        <div className="text-caption text-status-error">
          Not approved — nothing will run until you approve it
          {server.trustRevokedReason ? ` (${labelize(server.trustRevokedReason)})` : ""}
        </div>
      )}
      {missingSecrets.length > 0 && (
        <div className="text-caption text-status-warning">
          Missing secret values: {missingSecrets.join(", ")} — edit to fill them in.
        </div>
      )}
    </>
  );
}

function OAuthControls({
  saving,
  needsSignIn,
  signedIn,
  onSignIn,
  onSignOut,
}: {
  saving: boolean;
  needsSignIn: boolean;
  signedIn: boolean;
  onSignIn?: () => void;
  onSignOut?: () => void;
}) {
  if (needsSignIn && onSignIn) {
    return (
      <div className="pt-1">
        <Button type="button" size="xs" variant="outline" disabled={saving} onClick={onSignIn}>
          <KeyRound aria-hidden="true" />
          Sign in
        </Button>
      </div>
    );
  }
  if (signedIn) {
    return (
      <div className="flex items-center gap-2 pt-1">
        <span className="text-caption text-status-success">Signed in</span>
        {onSignOut && (
          <Button type="button" size="xs" variant="ghost" disabled={saving} onClick={onSignOut}>
            Sign out
          </Button>
        )}
      </div>
    );
  }
  return null;
}

function ToolChips({ inventory }: { inventory: McpServer["inventory"] }) {
  return (
    <div className="flex flex-wrap gap-1 pt-1">
      {inventory.map((tool) => (
        <span
          key={tool.name}
          className="rounded-sm border border-surface-overlay px-1.5 py-0.5 font-mono text-caption text-text-muted"
          title={tool.description ?? tool.name}
        >
          {tool.name}
          {tool.readOnly ? "" : "*"}
        </span>
      ))}
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
  onSignIn,
  onSignOut,
  onEdit,
  onDelete,
}: {
  server: McpServer;
  saving: boolean;
  onToggle: (enabled: boolean) => void;
  onRefresh: () => void;
  onTrust?: () => void;
  onRevokeTrust?: () => void;
  onSignIn?: () => void;
  onSignOut?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  return (
    <div className="rounded-md border border-surface-overlay bg-surface-elevated p-3">
      <div className="flex items-start justify-between gap-3">
        <ServerDetails server={server} saving={saving} onSignIn={onSignIn} onSignOut={onSignOut} />

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
              title="Reconnect"
              aria-label={`Reconnect ${server.displayName}`}
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
                    title="Revoke approval"
                    aria-label={`Revoke approval for ${server.displayName}`}
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
                    title="Approve server"
                    aria-label={`Approve ${server.displayName}`}
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

function PairRows({
  pairs,
  valuePlaceholder,
  addLabel,
  removeLabel,
  onChange,
}: {
  pairs: PairDraft[];
  valuePlaceholder: string;
  addLabel: string;
  removeLabel: string;
  onChange: (pairs: PairDraft[]) => void;
}) {
  const update = (id: string, patch: Partial<PairDraft>) => {
    onChange(pairs.map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)));
  };
  return (
    <div className="grid gap-2">
      {pairs.map((entry) => (
        <div key={entry.id} className="grid grid-cols-[1fr_1fr_auto_auto] gap-2">
          <input
            className={monoInputClass}
            value={entry.key}
            onChange={(event) => {
              // Secret-looking keys start out locked; the user can unlock.
              const key = event.target.value;
              update(entry.id, { key, secret: entry.secret || isLikelySecretKey(key) });
            }}
            placeholder="NAME"
            aria-label="Name"
          />
          <input
            className={`${monoInputClass} ${entry.needsValue ? "border-status-warning" : ""}`}
            type={entry.secret ? "password" : "text"}
            autoComplete="off"
            value={entry.value}
            onChange={(event) => update(entry.id, { value: event.target.value })}
            placeholder={
              entry.needsValue
                ? "value required"
                : entry.secret
                  ? "stored securely"
                  : valuePlaceholder
            }
            aria-label="Value"
          />
          <Button
            type="button"
            size="icon-sm"
            variant={entry.secret ? "default" : "ghost"}
            onClick={() => update(entry.id, { secret: !entry.secret })}
            title={entry.secret ? "Stored as secret" : "Mark as secret"}
            aria-label={entry.secret ? "Unmark as secret" : "Mark as secret"}
          >
            <Lock aria-hidden="true" />
          </Button>
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            onClick={() => onChange(pairs.filter((item) => item.id !== entry.id))}
            aria-label={removeLabel}
          >
            <X aria-hidden="true" />
          </Button>
        </div>
      ))}
      <div>
        <Button
          type="button"
          size="xs"
          variant="outline"
          onClick={() => onChange([...pairs, { id: rowId(), key: "", value: "", secret: false }])}
        >
          <Plus aria-hidden="true" />
          {addLabel}
        </Button>
      </div>
    </div>
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
  return (
    <div className="grid gap-3 rounded-md border border-accent-action bg-surface-elevated p-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h4 className="m-0 text-body-sm font-medium text-text-primary">
            {editing ? "Edit server" : "New server"}
          </h4>
          <p className="m-0 text-caption text-text-muted">
            {editing
              ? "Changing the command or URL revokes approval and reconnects."
              : "Adding approves and connects the server. Mutating tools ask before they run."}
          </p>
        </div>
        <Button type="button" size="icon-xs" variant="ghost" onClick={onCancel} aria-label="Cancel">
          <X aria-hidden="true" />
        </Button>
      </div>

      <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
        Paste to fill
        <textarea
          className={`${monoInputClass} min-h-[52px] resize-y`}
          value={draft.sourceText}
          onChange={(event) => onChange(applySourceText(draft, event.target.value))}
          placeholder={
            "https://api.mobbin.com/mcp\nor npx -y @modelcontextprotocol/server-everything\nor a JSON config from Claude / Cursor / OpenCode / VS Code"
          }
          aria-label="Paste server URL, command, or JSON config"
        />
      </label>

      <div className="grid grid-cols-[auto_1fr] items-center gap-2">
        <span className="rounded-sm bg-surface-overlay px-1.5 py-0.5 font-mono text-caption text-text-muted">
          {draft.transport === "http" ? "http" : "stdio"}
        </span>
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          Name
          <input
            className={inputClass}
            value={draft.displayName}
            onChange={(event) =>
              onChange({ ...draft, displayName: event.target.value, nameTouched: true })
            }
            placeholder={suggestName(draft.transport, draft.url, draft.commandLine) || "server"}
          />
        </label>
      </div>

      {draft.transport === "http" ? (
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          URL
          <input
            className={monoInputClass}
            value={draft.url}
            onChange={(event) => onChange({ ...draft, url: event.target.value })}
            placeholder="https://api.mobbin.com/mcp"
          />
        </label>
      ) : (
        <label className="grid gap-1 text-caption font-medium uppercase text-text-muted">
          Command
          <input
            className={monoInputClass}
            value={draft.commandLine}
            onChange={(event) => onChange({ ...draft, commandLine: event.target.value })}
            placeholder="npx -y @modelcontextprotocol/server-everything"
          />
        </label>
      )}

      <button
        type="button"
        className="flex items-center gap-1 border-none bg-transparent p-0 text-left text-body-sm text-text-secondary"
        onClick={() => onChange({ ...draft, showAdvanced: !draft.showAdvanced })}
      >
        <ChevronDown
          aria-hidden="true"
          className={`size-4 transition-transform ${draft.showAdvanced ? "rotate-180" : ""}`}
        />
        {draft.transport === "http" ? "Headers" : "Environment"}
      </button>

      {draft.showAdvanced && (
        <div className="grid gap-2">
          <p className="m-0 text-caption text-text-muted">
            {draft.transport === "http"
              ? "Locked headers are stored in the credential store and sent with every request. OAuth servers sign in through the browser after you add them."
              : "Locked values are stored in the credential store and passed to the process only."}
          </p>
          {draft.transport === "http" ? (
            <PairRows
              pairs={draft.headers}
              valuePlaceholder="value"
              addLabel="Header"
              removeLabel="Remove header"
              onChange={(headers) => onChange({ ...draft, headers })}
            />
          ) : (
            <PairRows
              pairs={draft.env}
              valuePlaceholder="value"
              addLabel="Variable"
              removeLabel="Remove variable"
              onChange={(env) => onChange({ ...draft, env })}
            />
          )}
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
          {saving ? "Working" : editing ? "Save and reconnect" : "Add and connect"}
        </Button>
      </div>
    </div>
  );
}
