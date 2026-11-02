import type { CreateMcpServerRequest, McpEnvValue, McpTransport } from "../types";

export interface PairDraft {
  id: string;
  key: string;
  value: string;
  /** Values marked secret go to the credential store, never the config DB. */
  secret: boolean;
  /** Declared but has no stored value — the connect will fail without one. */
  needsValue?: boolean;
}

export interface ServerDraft {
  /** Whatever the user pasted: a URL, a command line, or a JSON config. */
  sourceText: string;
  transport: McpTransport;
  displayName: string;
  /** Once the user edits the name we stop auto-suggesting from the source. */
  nameTouched: boolean;
  /** stdio: single command line, e.g. `npx -y @scope/server`. */
  commandLine: string;
  /** http: endpoint URL. */
  url: string;
  env: PairDraft[];
  headers: PairDraft[];
  showAdvanced: boolean;
}

export function rowId() {
  return Math.random().toString(36).slice(2);
}

export function emptyDraft(): ServerDraft {
  return {
    sourceText: "",
    transport: "stdio",
    displayName: "",
    nameTouched: false,
    commandLine: "",
    url: "",
    env: [],
    headers: [],
    showAdvanced: false,
  };
}

export function isLikelySecretKey(key: string) {
  const upper = key.toUpperCase();
  return (
    upper === "AUTHORIZATION" ||
    ["TOKEN", "SECRET", "PASSWORD", "API_KEY", "ACCESS_KEY"].some((needle) =>
      upper.includes(needle)
    )
  );
}

export function pairDrafts(
  entries: McpEnvValue[],
  secretKeys: string[],
  missingKeys: string[]
): PairDraft[] {
  const missing = new Set(missingKeys);
  const pairs: PairDraft[] = entries.map((entry) => ({
    id: rowId(),
    key: entry.key,
    value: entry.value,
    secret: false,
    needsValue: false,
  }));
  for (const key of secretKeys) {
    // Values are never sent back to the frontend; a blank field keeps the
    // stored secret on save, so we only flag keys with nothing stored.
    pairs.push({ id: rowId(), key, value: "", secret: true, needsValue: missing.has(key) });
  }
  return pairs;
}

/** Quote-aware whitespace split: 'a b' and "a b" stay together. */
export function splitCommandLine(text: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  for (const ch of text) {
    if (quote) {
      if (ch === quote) {
        quote = null;
      } else {
        current += ch;
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
    } else if (/\s/.test(ch)) {
      if (current) {
        tokens.push(current);
        current = "";
      }
    } else {
      current += ch;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

/** Suggest a display name: hostname for URLs, last non-flag arg's basename for commands. */
export function suggestName(transport: McpTransport, url: string, commandLine: string): string {
  if (transport === "http") {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return "";
    }
  }
  const tokens = splitCommandLine(commandLine).filter((token) => !token.startsWith("-"));
  const candidate = tokens[tokens.length - 1] ?? "";
  return candidate.replace(/^@[^/]+\//, "").replace(/[^a-zA-Z0-9_-].*$/, "");
}

export interface ParsedEntry {
  transport: McpTransport;
  url?: string;
  command?: string;
  args?: string[];
  env?: McpEnvValue[];
  headers?: McpEnvValue[];
}

/** Find the first server entry in a pasted MCP config object. */
export function entryFromJson(value: unknown): { name?: string; entry: ParsedEntry } | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const mcp = record.mcp as Record<string, unknown> | undefined;
  const maps = [record.mcpServers, mcp?.mcpServers, mcp?.servers, record.servers, mcp];
  for (const map of maps) {
    if (typeof map !== "object" || map === null) continue;
    for (const [name, config] of Object.entries(map as Record<string, unknown>)) {
      const entry = parseServerEntry(config);
      if (entry) return { name, entry };
    }
  }
  const single = parseServerEntry(value);
  return single ? { entry: single } : null;
}

function parseServerEntry(config: unknown): ParsedEntry | null {
  if (typeof config !== "object" || config === null) return null;
  const record = config as Record<string, unknown>;
  const type = typeof record.type === "string" ? record.type.toLowerCase() : "";
  const url = typeof record.url === "string" ? record.url.trim() : "";
  if (url && type !== "local" && type !== "stdio") {
    return {
      transport: "http",
      url,
      headers: objectToPairs(record.headers),
    };
  }
  let command = "";
  let args: string[] = [];
  if (typeof record.command === "string") {
    command = record.command;
  } else if (Array.isArray(record.command)) {
    const parts = record.command.filter((part): part is string => typeof part === "string");
    command = parts[0] ?? "";
    args = parts.slice(1);
  }
  if (Array.isArray(record.args)) {
    args = args.concat(record.args.filter((part): part is string => typeof part === "string"));
  }
  if (!command) return null;
  return {
    transport: "stdio",
    command,
    args,
    env: objectToPairs(record.env ?? record.environment),
  };
}

function objectToPairs(value: unknown): McpEnvValue[] {
  if (typeof value !== "object" || value === null) return [];
  return Object.entries(value as Record<string, unknown>).map(([key, entryValue]) => ({
    key,
    value: typeof entryValue === "string" ? entryValue : String(entryValue ?? ""),
  }));
}

function toPairDrafts(entries: McpEnvValue[]): PairDraft[] {
  return entries.map((pair) => ({
    id: rowId(),
    key: pair.key,
    value: pair.value,
    secret: isLikelySecretKey(pair.key),
    needsValue: false,
  }));
}

/** Apply whatever the user pasted to the draft's transport-specific fields. */
export function applySourceText(draft: ServerDraft, text: string): ServerDraft {
  const trimmed = text.trim();
  const next = { ...draft, sourceText: text };
  if (!trimmed) return next;

  if (/^https?:\/\/\S+$/i.test(trimmed)) {
    return {
      ...next,
      transport: "http",
      url: trimmed,
      displayName: draft.nameTouched ? draft.displayName : suggestName("http", trimmed, ""),
    };
  }

  if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
    try {
      const parsed = entryFromJson(JSON.parse(trimmed));
      if (!parsed) return next; // valid JSON, no server entry — keep typing
      const { entry, name } = parsed;
      const env = toPairDrafts(entry.env ?? []);
      const headers = toPairDrafts(entry.headers ?? []);
      return {
        ...next,
        transport: entry.transport,
        url: entry.url ?? "",
        commandLine:
          entry.transport === "stdio"
            ? [entry.command ?? "", ...(entry.args ?? [])].filter(Boolean).join(" ")
            : "",
        env,
        headers,
        showAdvanced: env.length + headers.length > 0,
        displayName: draft.nameTouched
          ? draft.displayName
          : (name ?? suggestName(entry.transport, entry.url ?? "", entry.command ?? "")),
      };
    } catch {
      return next; // still typing JSON
    }
  }

  return {
    ...next,
    transport: "stdio",
    commandLine: trimmed,
    displayName: draft.nameTouched ? draft.displayName : suggestName("stdio", "", trimmed),
  };
}

export function requestFromDraft(draft: ServerDraft): CreateMcpServerRequest {
  if (draft.transport === "http") {
    return {
      displayName: draft.displayName.trim(),
      transport: "http",
      url: draft.url.trim(),
      headers: draft.headers
        .filter((entry) => entry.key.trim() && !entry.secret)
        .map((entry) => ({ key: entry.key.trim(), value: entry.value })),
      secretHeaders: draft.headers
        .filter((entry) => entry.key.trim() && entry.secret)
        .map((entry) => ({ key: entry.key.trim(), value: entry.value })),
      scope: "main_and_exploration",
    };
  }
  const tokens = splitCommandLine(draft.commandLine);
  return {
    displayName: draft.displayName.trim(),
    transport: "stdio",
    command: tokens[0] ?? "",
    args: tokens.slice(1),
    env: draft.env
      .filter((entry) => entry.key.trim() && !entry.secret)
      .map((entry) => ({ key: entry.key.trim(), value: entry.value })),
    secretEnv: draft.env
      .filter((entry) => entry.key.trim() && entry.secret)
      .map((entry) => ({ key: entry.key.trim(), value: entry.value })),
    scope: "main_and_exploration",
  };
}

export function validationErrors(draft: ServerDraft) {
  const request = requestFromDraft(draft);
  const errors: string[] = [];
  if (!request.displayName) errors.push("Name is required.");
  if (draft.transport === "http") {
    const url = request.url ?? "";
    if (!url) {
      errors.push("URL is required.");
    } else if (!/^https?:\/\//i.test(url)) {
      errors.push("URL must start with https:// or http://.");
    }
    checkPairs(draft.headers, "header", errors);
  } else {
    if (!request.command) errors.push("Command is required.");
    checkPairs(draft.env, "env key", errors);
  }
  return errors;
}

function checkPairs(pairs: PairDraft[], label: string, errors: string[]) {
  const secretKeys = pairs.filter((p) => p.secret && p.key.trim()).map((p) => p.key.trim());
  const plainKeys = pairs.filter((p) => !p.secret && p.key.trim()).map((p) => p.key.trim());
  const dupe = firstDuplicate(plainKeys);
  const secretDupe = firstDuplicate(secretKeys);
  const overlap = plainKeys.find((key) => secretKeys.includes(key));
  if (dupe) errors.push(`Duplicate ${label}: ${dupe}.`);
  if (secretDupe) errors.push(`Duplicate secret ${label}: ${secretDupe}.`);
  if (overlap) errors.push(`${overlap} cannot be both secret and non-secret.`);
}

function firstDuplicate(values: string[]) {
  const seen = new Set<string>();
  return values.find((value) => {
    if (seen.has(value)) return true;
    seen.add(value);
    return false;
  });
}
